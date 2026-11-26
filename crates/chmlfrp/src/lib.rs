pub mod auth;

use anyhow::{Context, Result, anyhow};
use ashan_frp_domain::{NodePolicyFlags, NodeSummary, TunnelPlan};
use auth::{SsoTokenBundle, login_via_qzhua_sso, now_epoch_ms, refresh_sso_token};
use futures_util::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::Mutex;
use tracing::{info, warn};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const TOKEN_REFRESH_MARGIN_MS: i64 = 60_000;

pub type TokenPersister =
    Arc<dyn Fn(SsoTokenBundle) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ChmlFrpConfig {
    pub base_url: String,
    pub token: String,
    pub refresh_token: String,
    pub token_expires_at: i64,
    pub username: String,
    pub password: String,
    pub totp_secret: String,
}

#[derive(Clone)]
pub struct ChmlFrpClient {
    http: Client,
    config: Arc<RwLock<ChmlFrpConfig>>,
    auth_lock: Arc<Mutex<()>>,
    persister: Arc<RwLock<Option<TokenPersister>>>,
}

impl ChmlFrpClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        Self::new_with_auth(ChmlFrpConfig {
            base_url: base_url.into(),
            token: token.into(),
            refresh_token: String::new(),
            token_expires_at: 0,
            username: String::new(),
            password: String::new(),
            totp_secret: String::new(),
        })
    }

    pub fn new_with_auth(mut cfg: ChmlFrpConfig) -> Result<Self> {
        cfg.base_url = normalize_base_url(cfg.base_url);
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("build ChmlFrp HTTP client")?;

        Ok(Self {
            http,
            config: Arc::new(RwLock::new(cfg)),
            auth_lock: Arc::new(Mutex::new(())),
            persister: Arc::new(RwLock::new(None)),
        })
    }

    pub fn set_token_persister(&self, hook: TokenPersister) {
        let mut guard = self
            .persister
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(hook);
    }

    fn snapshot(&self) -> ChmlFrpConfig {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn reconfigure(&self, base_url: impl Into<String>, token: impl Into<String>) {
        let mut config = self
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config.base_url = normalize_base_url(base_url.into());
        config.token = token.into();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reconfigure_full(
        &self,
        base_url: impl Into<String>,
        token: impl Into<String>,
        refresh_token: impl Into<String>,
        token_expires_at: i64,
        username: impl Into<String>,
        password: impl Into<String>,
        totp_secret: impl Into<String>,
    ) {
        let mut config = self
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config.base_url = normalize_base_url(base_url.into());
        config.token = token.into();
        config.refresh_token = refresh_token.into();
        config.token_expires_at = token_expires_at;
        config.username = username.into();
        config.password = password.into();
        config.totp_secret = totp_secret.into();
    }

    pub fn configured(&self) -> bool {
        let cfg = self.snapshot();
        !cfg.token.trim().is_empty()
            || !cfg.refresh_token.trim().is_empty()
            || (!cfg.username.trim().is_empty() && !cfg.password.is_empty())
    }

    fn can_auto_renew(cfg: &ChmlFrpConfig) -> bool {
        !cfg.refresh_token.trim().is_empty()
            || (!cfg.username.trim().is_empty() && !cfg.password.is_empty())
    }

    fn token_fresh(cfg: &ChmlFrpConfig) -> bool {
        if cfg.token.trim().is_empty() {
            return false;
        }
        if cfg.token_expires_at <= 0 {
            return true;
        }
        now_epoch_ms() + TOKEN_REFRESH_MARGIN_MS < cfg.token_expires_at
    }

    /// Ensure an active ChmlFrp access token is available, automatically refreshing
    /// via `/sso/refresh` or logging in via Qzhua SSO + TOTP 2FA if needed.
    pub async fn ensure_access_token(&self) -> Result<String> {
        let cfg = self.snapshot();
        if Self::token_fresh(&cfg) {
            return Ok(cfg.token);
        }
        if !Self::can_auto_renew(&cfg) {
            if !cfg.token.trim().is_empty() {
                return Ok(cfg.token);
            }
            return Err(anyhow!("ChmlFrp token or credentials not configured"));
        }
        self.refresh_or_relogin(&cfg.token, false).await
    }

    /// Explicitly force SSO login / refresh right now and persist the resulting tokens.
    pub async fn login_now(&self) -> Result<SsoTokenBundle> {
        let _guard = self.auth_lock.lock().await;
        let cfg = self.snapshot();
        let bundle = self.obtain_tokens_locked(&cfg, true).await?;
        self.apply_token_bundle(&bundle).await;
        Ok(bundle)
    }

    async fn refresh_or_relogin(&self, stale_token: &str, force_relogin: bool) -> Result<String> {
        let _guard = self.auth_lock.lock().await;
        let cfg = self.snapshot();
        if !force_relogin && cfg.token != stale_token && Self::token_fresh(&cfg) {
            return Ok(cfg.token);
        }
        let bundle = self.obtain_tokens_locked(&cfg, false).await?;
        self.apply_token_bundle(&bundle).await;
        Ok(bundle.access_token)
    }

    async fn obtain_tokens_locked(
        &self,
        cfg: &ChmlFrpConfig,
        prefer_credentials: bool,
    ) -> Result<SsoTokenBundle> {
        if !prefer_credentials && !cfg.refresh_token.trim().is_empty() {
            match refresh_sso_token(&self.http, &cfg.base_url, &cfg.refresh_token).await {
                Ok(bundle) => {
                    info!("ChmlFrp SSO token refreshed via /sso/refresh");
                    return Ok(bundle);
                }
                Err(err) => {
                    warn!(error = %err, "ChmlFrp /sso/refresh failed, falling back to SSO credentials");
                }
            }
        }

        if !cfg.username.trim().is_empty() && !cfg.password.is_empty() {
            let totp = (!cfg.totp_secret.trim().is_empty()).then_some(cfg.totp_secret.as_str());
            match login_via_qzhua_sso(&cfg.base_url, &cfg.username, &cfg.password, totp).await {
                Ok(bundle) => {
                    info!(username = %cfg.username, "ChmlFrp SSO login succeeded via Qzhua + MFA");
                    return Ok(bundle);
                }
                Err(sso_err) => {
                    // Fallback for custom self-hosted mirrors using legacy /login.
                    if sso_err.to_string().contains("未返回 OAuth2 重定向")
                        && let Ok(bundle) = self
                            .legacy_login(&cfg.base_url, &cfg.username, &cfg.password)
                            .await
                    {
                        return Ok(bundle);
                    }
                    return Err(sso_err);
                }
            }
        }

        if !cfg.refresh_token.trim().is_empty() {
            return refresh_sso_token(&self.http, &cfg.base_url, &cfg.refresh_token).await;
        }

        Err(anyhow!(
            "ChmlFrp Token 已失效，且未配置可用的 Refresh Token 或轻爪账号密码"
        ))
    }

    async fn legacy_login(
        &self,
        base_url: &str,
        username: &str,
        password: &str,
    ) -> Result<SsoTokenBundle> {
        #[derive(Deserialize)]
        struct LegacyLoginData {
            usertoken: String,
        }
        let resp: Envelope<LegacyLoginData> = self
            .http
            .post(format!("{base_url}/login"))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure_success(&resp.state, resp.code, &resp.msg)?;
        let data = resp
            .data
            .ok_or_else(|| anyhow!("legacy /login returned no data"))?;
        Ok(SsoTokenBundle {
            access_token: data.usertoken,
            refresh_token: None,
            expires_in: 0,
            expires_at_ms: 0,
        })
    }

    async fn apply_token_bundle(&self, bundle: &SsoTokenBundle) {
        {
            let mut config = self
                .config
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            config.token = bundle.access_token.clone();
            if let Some(ref rt) = bundle.refresh_token
                && !rt.trim().is_empty()
            {
                config.refresh_token = rt.clone();
            }
            config.token_expires_at = bundle.expires_at_ms;
        }
        let hook = self
            .persister
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(persist) = hook
            && let Err(err) = persist(bundle.clone()).await
        {
            warn!(error = %err, "failed to persist refreshed ChmlFrp tokens to SQLite");
        }
    }

    async fn with_authed_retry<T, F, Fut>(&self, mut op: F) -> Result<T>
    where
        F: FnMut(String, String) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let base_url = self.snapshot().base_url;
        let token = self.ensure_access_token().await?;
        match op(base_url.clone(), token.clone()).await {
            Ok(val) => Ok(val),
            Err(err) if is_auth_error(&err) && Self::can_auto_renew(&self.snapshot()) => {
                warn!(error = %err, "ChmlFrp API auth error, refreshing session and retrying");
                let fresh = self.refresh_or_relogin(&token, false).await?;
                op(base_url, fresh).await
            }
            Err(err) => Err(err),
        }
    }

    pub async fn userinfo(&self) -> Result<ChmlUserInfo> {
        self.with_authed_retry(|base_url, token| {
            let http = self.http.clone();
            async move {
                let response: Envelope<ChmlUserInfo> = http
                    .get(format!("{base_url}/userinfo"))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .context("read ChmlFrp userinfo")?
                    .error_for_status()
                    .context("ChmlFrp userinfo HTTP status")?
                    .json()
                    .await
                    .context("decode ChmlFrp userinfo response")?;
                ensure_success(&response.state, response.code, &response.msg)?;
                response
                    .data
                    .ok_or_else(|| anyhow!("ChmlFrp userinfo returned no data"))
            }
        })
        .await
    }

    pub async fn list_tunnels(&self) -> Result<Vec<RemoteTunnel>> {
        self.with_authed_retry(|base_url, token| {
            let http = self.http.clone();
            async move {
                let response: Envelope<Vec<RemoteTunnel>> = http
                    .get(format!("{base_url}/tunnel"))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .context("list ChmlFrp tunnels")?
                    .error_for_status()
                    .context("ChmlFrp tunnel-list HTTP status")?
                    .json()
                    .await
                    .context("decode ChmlFrp tunnel-list response")?;
                ensure_success(&response.state, response.code, &response.msg)?;
                Ok(response.data.unwrap_or_default())
            }
        })
        .await
    }

    pub async fn list_nodes(&self) -> Result<Vec<NodeSummary>> {
        let base_url = self.snapshot().base_url;
        let response: Envelope<Vec<NodeListItem>> = self
            .http
            .get(format!("{base_url}/node"))
            .send()
            .await
            .context("list ChmlFrp nodes")?
            .error_for_status()
            .context("ChmlFrp node-list HTTP status")?
            .json()
            .await
            .context("decode ChmlFrp node-list response")?;
        ensure_success(&response.state, response.code, &response.msg)?;

        let items = response.data.unwrap_or_default();
        if !self.configured() {
            return Ok(items
                .into_iter()
                .map(|item| build_node_summary(Some(&item), None))
                .collect());
        }

        // Ensure valid token once before fanning out nodeinfo requests concurrently.
        let _ = self.ensure_access_token().await?;
        let details = join_all(items.iter().map(|item| {
            let client = self.clone();
            let name = item.name.clone();
            async move { client.node_info(&name).await.ok() }
        }))
        .await;

        let nodes = items
            .into_iter()
            .zip(details)
            .map(|(item, detail)| build_node_summary(Some(&item), detail.as_ref()))
            .collect();
        Ok(nodes)
    }

    pub async fn node_summary(&self, node: &str) -> Result<NodeSummary> {
        let mut info = self.node_info(node).await?;
        if info.name.trim().is_empty() {
            info.name = node.to_owned();
        }
        Ok(build_node_summary(None, Some(&info)))
    }

    pub async fn node_info(&self, node: &str) -> Result<NodeInfo> {
        let node_name = node.to_owned();
        self.with_authed_retry(move |base_url, token| {
            let http = self.http.clone();
            let node = node_name.clone();
            async move {
                let response: Envelope<NodeInfo> = http
                    .get(format!("{base_url}/nodeinfo"))
                    .bearer_auth(&token)
                    .query(&[("node", node.as_str())])
                    .send()
                    .await
                    .with_context(|| format!("read ChmlFrp node {node}"))?
                    .error_for_status()
                    .with_context(|| format!("ChmlFrp nodeinfo HTTP status for {node}"))?
                    .json()
                    .await
                    .with_context(|| format!("decode ChmlFrp nodeinfo response for {node}"))?;
                ensure_success(&response.state, response.code, &response.msg)?;
                response
                    .data
                    .ok_or_else(|| anyhow!("ChmlFrp nodeinfo returned no data"))
            }
        })
        .await
    }

    pub async fn create_tunnel(
        &self,
        plan: &TunnelPlan,
        target_node: &str,
    ) -> Result<RemoteTunnel> {
        let usertoken = self
            .userinfo()
            .await
            .map(|u| u.usertoken)
            .unwrap_or_default();
        let plan = plan.clone();
        let target_node = target_node.to_owned();
        self.with_authed_retry(move |base_url, token| {
            let http = self.http.clone();
            let payload = TunnelCreatePayload::from_plan(&usertoken, &plan, &target_node, 0);
            let name = plan.name.clone();
            async move {
                let response: Envelope<RemoteTunnel> = http
                    .post(format!("{base_url}/create_tunnel"))
                    .bearer_auth(&token)
                    .json(&payload)
                    .send()
                    .await
                    .with_context(|| format!("create ChmlFrp tunnel {name}"))?
                    .error_for_status()
                    .with_context(|| format!("ChmlFrp create_tunnel HTTP status for {name}"))?
                    .json()
                    .await
                    .with_context(|| format!("decode ChmlFrp create_tunnel response for {name}"))?;
                ensure_success(&response.state, response.code, &response.msg)?;
                response
                    .data
                    .ok_or_else(|| anyhow!("ChmlFrp create_tunnel returned no data"))
            }
        })
        .await
    }

    pub async fn create_remote_tunnel(&self, input: &RemoteTunnelMutation) -> Result<RemoteTunnel> {
        let usertoken = self
            .userinfo()
            .await
            .map(|u| u.usertoken)
            .unwrap_or_default();
        let input = input.clone();
        self.with_authed_retry(move |base_url, token| {
            let http = self.http.clone();
            let payload = TunnelCreatePayload::from_remote_input(&usertoken, &input);
            let name = input.tunnel_name.clone();
            async move {
                let response: Envelope<RemoteTunnel> = http
                    .post(format!("{base_url}/create_tunnel"))
                    .bearer_auth(&token)
                    .json(&payload)
                    .send()
                    .await
                    .with_context(|| format!("create ChmlFrp tunnel {name}"))?
                    .error_for_status()
                    .with_context(|| format!("ChmlFrp create_tunnel HTTP status for {name}"))?
                    .json()
                    .await
                    .with_context(|| format!("decode ChmlFrp create_tunnel response for {name}"))?;
                ensure_success(&response.state, response.code, &response.msg)?;
                response
                    .data
                    .ok_or_else(|| anyhow!("ChmlFrp create_tunnel returned no data"))
            }
        })
        .await
    }

    pub async fn update_remote_tunnel(
        &self,
        tunnel_id: i64,
        input: &RemoteTunnelMutation,
    ) -> Result<()> {
        let input = input.clone();
        self.with_authed_retry(move |base_url, token| {
            let http = self.http.clone();
            let payload = TunnelUpdatePayload::from_remote_input(tunnel_id, &input);
            let name = input.tunnel_name.clone();
            async move {
                let response: Envelope<serde_json::Value> = http
                    .post(format!("{base_url}/update_tunnel"))
                    .bearer_auth(&token)
                    .json(&payload)
                    .send()
                    .await
                    .with_context(|| format!("update ChmlFrp tunnel {name}"))?
                    .error_for_status()
                    .with_context(|| format!("ChmlFrp update_tunnel HTTP status for {name}"))?
                    .json()
                    .await
                    .with_context(|| format!("decode ChmlFrp update_tunnel response for {name}"))?;
                ensure_success(&response.state, response.code, &response.msg)
            }
        })
        .await
    }

    pub const fn delete_tunnel_supported(&self) -> bool {
        false
    }

    /// Reconcile one existing provider resource to the control-center plan and the
    /// single global target node. This is a provider operation; failover remains
    /// global and never calls this method for one tunnel in isolation.
    pub async fn sync_tunnel(
        &self,
        remote: &RemoteTunnel,
        plan: &TunnelPlan,
        target_node: &str,
    ) -> Result<()> {
        let remote = remote.clone();
        let plan = plan.clone();
        let target_node = target_node.to_owned();
        self.with_authed_retry(move |base_url, token| {
            let http = self.http.clone();
            let payload = TunnelUpdatePayload::from_plan_and_remote(&remote, &plan, &target_node);
            let name = plan.name.clone();
            let node = target_node.clone();
            async move {
                let response: Envelope<serde_json::Value> = http
                    .post(format!("{base_url}/update_tunnel"))
                    .bearer_auth(&token)
                    .json(&payload)
                    .send()
                    .await
                    .with_context(|| format!("sync ChmlFrp tunnel {name}"))?
                    .error_for_status()
                    .with_context(|| format!("ChmlFrp update_tunnel HTTP status for {name}"))?
                    .json()
                    .await
                    .with_context(|| format!("decode ChmlFrp update_tunnel response for {name}"))?;
                ensure_success(&response.state, response.code, &response.msg)
                    .with_context(|| format!("sync tunnel {name} to global node {node}"))
            }
        })
        .await
    }

    pub async fn generated_config(&self, node: &str, tunnel_names: &[String]) -> Result<String> {
        let node = node.to_owned();
        let names = tunnel_names.join(",");
        self.with_authed_retry(move |base_url, token| {
            let http = self.http.clone();
            let node = node.clone();
            let names = names.clone();
            async move {
                let response: Envelope<String> = http
                    .get(format!("{base_url}/tunnel_config"))
                    .bearer_auth(&token)
                    .query(&[("node", node.as_str()), ("tunnel_names", names.as_str())])
                    .send()
                    .await
                    .with_context(|| format!("generate ChmlFrp config for node {node}"))?
                    .error_for_status()
                    .with_context(|| format!("ChmlFrp tunnel_config HTTP status for {node}"))?
                    .json()
                    .await
                    .with_context(|| format!("decode ChmlFrp tunnel_config response for {node}"))?;
                ensure_success(&response.state, response.code, &response.msg)?;
                response
                    .data
                    .ok_or_else(|| anyhow!("ChmlFrp tunnel_config returned no config"))
            }
        })
        .await
    }

    pub async fn health(&self) -> Result<()> {
        self.list_tunnels().await.map(|_| ())
    }
}

fn is_auth_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    msg.contains("code=401")
        || msg.contains("401 Unauthorized")
        || msg.contains("无效的登录状态")
        || msg.contains("缺少登录态")
}

fn normalize_base_url(value: String) -> String {
    value.trim().trim_end_matches('/').to_owned()
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn ensure_success(state: &str, code: i64, message: &str) -> Result<()> {
    if state.eq_ignore_ascii_case("success") || code == 200 {
        Ok(())
    } else {
        Err(anyhow!(
            "ChmlFrp error code={code} state={state}: {message}"
        ))
    }
}

fn parse_boolish(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    msg: String,
    code: i64,
    state: String,
    data: Option<T>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChmlUserInfo {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub usertoken: String,
    #[serde(default)]
    pub usergroup: String,
}

fn parse_optional_boolish(value: &str) -> Option<bool> {
    let trimmed = value.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn build_node_summary(item: Option<&NodeListItem>, detail: Option<&NodeInfo>) -> NodeSummary {
    let id = item
        .map(|i| i.id)
        .or_else(|| detail.map(|d| d.id))
        .unwrap_or_default();
    let name = item
        .map(|i| i.name.clone())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| detail.map(|d| d.name.clone()))
        .unwrap_or_default();
    let area = item
        .map(|i| i.area.clone())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| detail.map(|d| d.area.clone()))
        .unwrap_or_default();
    let web_str = detail
        .map(|d| d.web.as_str())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| item.map(|i| i.web.as_str()))
        .unwrap_or("");
    let state = detail
        .map(|d| d.state.clone())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| item.and_then(|i| nonempty(i.state.clone())))
        .unwrap_or_else(|| "unknown".to_owned());
    let notes = detail
        .map(|d| d.notes.clone())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| item.map(|i| i.notes.clone()))
        .unwrap_or_default();
    let nodegroup = detail
        .map(|d| d.nodegroup.clone())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| item.and_then(|i| nonempty(i.nodegroup.clone())));
    let china = detail
        .and_then(|d| parse_optional_boolish(&d.china))
        .or_else(|| item.and_then(|i| parse_optional_boolish(&i.china)));
    let fangyu = detail
        .and_then(|d| parse_optional_boolish(&d.fangyu))
        .or_else(|| item.and_then(|i| parse_optional_boolish(&i.fangyu)));
    let udp = detail
        .and_then(|d| parse_optional_boolish(&d.udp))
        .or_else(|| item.and_then(|i| parse_optional_boolish(&i.udp)));

    let mut summary = NodeSummary {
        id,
        name,
        area,
        web_supported: web_str.eq_ignore_ascii_case("yes") || parse_boolish(web_str),
        state,
        real_ip: detail.and_then(|d| nonempty(d.real_ip.clone())),
        host: detail.and_then(|d| nonempty(d.ip.clone())),
        frps_port: detail.and_then(|d| u16::try_from(d.port).ok().filter(|&p| p > 0)),
        nodegroup,
        china,
        fangyu,
        udp,
        toowhite: detail.and_then(|d| d.toowhite),
        load1: detail.map(|d| d.load1),
        load5: detail.map(|d| d.load5),
        load15: detail.map(|d| d.load15),
        bandwidth_usage_percent: detail.map(|d| d.bandwidth_usage_percent),
        notes,
        tags: Vec::new(),
        policy: NodePolicyFlags::default(),
        quarantined_until: None,
    };
    summary.classify();
    summary
}

#[derive(Debug, Clone, Deserialize)]
struct NodeListItem {
    id: i64,
    name: String,
    area: String,
    web: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    nodegroup: String,
    #[serde(default)]
    china: String,
    #[serde(default)]
    fangyu: String,
    #[serde(default)]
    udp: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeInfo {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub area: String,
    #[serde(default)]
    pub state: String,
    #[serde(default, rename = "realIp")]
    pub real_ip: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub port: i64,
    #[serde(default)]
    pub load1: f64,
    #[serde(default)]
    pub load5: f64,
    #[serde(default)]
    pub load15: f64,
    #[serde(default)]
    pub bandwidth_usage_percent: f64,
    #[serde(default)]
    pub web: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub nodegroup: String,
    #[serde(default)]
    pub china: String,
    #[serde(default)]
    pub fangyu: String,
    #[serde(default)]
    pub udp: String,
    #[serde(default)]
    pub toowhite: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTunnelMutation {
    pub tunnel_name: String,
    pub node: String,
    pub port_type: String,
    pub local_ip: String,
    pub local_port: i64,
    #[serde(default)]
    pub remote_port: i64,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub encryption: bool,
    #[serde(default = "default_compression")]
    pub compression: bool,
    #[serde(default)]
    pub extra_params: String,
}

fn default_compression() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTunnel {
    #[serde(default, rename = "tunnelID", alias = "id")]
    pub tunnel_id: i64,
    #[serde(default, rename = "tunnelName", alias = "name")]
    pub tunnel_name: String,
    #[serde(default)]
    pub node: String,
    #[serde(default, rename = "localIP", alias = "localip")]
    pub local_ip: String,
    #[serde(default, rename = "localPort", alias = "nport")]
    pub local_port: i64,
    #[serde(
        default,
        rename = "remotePort",
        alias = "dorp",
        deserialize_with = "deserialize_string_or_number"
    )]
    pub remote_endpoint: String,
    #[serde(default, rename = "portType", alias = "type")]
    pub port_type: String,
    #[serde(default, rename = "bandDomain")]
    pub band_domain: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_bool")]
    pub encryption: String,
    #[serde(default, deserialize_with = "deserialize_string_or_bool")]
    pub compression: String,
    #[serde(default, rename = "extraParams", alias = "ap")]
    pub extra_params: Option<String>,
    #[serde(default, rename = "tunnelState", alias = "state")]
    pub tunnel_state: String,
}

impl RemoteTunnel {
    fn remote_port_number(&self) -> i64 {
        self.remote_endpoint.parse().unwrap_or(0)
    }

    pub fn effective_domain(&self) -> String {
        self.band_domain.clone().unwrap_or_else(|| {
            if matches!(
                self.port_type.to_ascii_lowercase().as_str(),
                "http" | "https"
            ) {
                self.remote_endpoint.clone()
            } else {
                String::new()
            }
        })
    }

    pub fn matches_plan_on_node(&self, plan: &TunnelPlan, node: &str) -> bool {
        let is_web = matches!(
            plan.protocol.to_ascii_lowercase().as_str(),
            "http" | "https"
        );
        self.node == node
            && self.local_ip == plan.local_ip
            && self.local_port == plan.local_port
            && self.port_type.eq_ignore_ascii_case(&plan.protocol)
            && (!is_web || self.effective_domain().eq_ignore_ascii_case(&plan.domain))
    }
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(value) => value,
        serde_json::Value::Number(value) => value.to_string(),
        other => other.to_string(),
    })
}

fn deserialize_string_or_bool<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(value) => value,
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        other => other.to_string(),
    })
}

#[derive(Debug, Serialize)]
struct TunnelCreatePayload {
    #[serde(skip_serializing_if = "String::is_empty")]
    token: String,
    tunnelname: String,
    node: String,
    porttype: String,
    localip: String,
    localport: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    remoteport: Option<i64>,
    encryption: bool,
    compression: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    extraparams: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    banddomain: Option<String>,
}

impl TunnelCreatePayload {
    fn from_remote_input(usertoken: &str, input: &RemoteTunnelMutation) -> Self {
        let porttype = input.port_type.trim().to_ascii_lowercase();
        let is_web = matches!(porttype.as_str(), "http" | "https");
        Self {
            token: usertoken.to_owned(),
            tunnelname: input.tunnel_name.trim().to_owned(),
            node: input.node.trim().to_owned(),
            porttype,
            localip: input.local_ip.trim().to_owned(),
            localport: input.local_port,
            remoteport: (!is_web).then_some(input.remote_port),
            encryption: input.encryption,
            compression: input.compression,
            extraparams: (!input.extra_params.trim().is_empty())
                .then(|| input.extra_params.clone()),
            banddomain: is_web.then(|| input.domain.trim().to_owned()),
        }
    }

    fn from_plan(usertoken: &str, plan: &TunnelPlan, node: &str, remote_port: i64) -> Self {
        let porttype = plan.protocol.to_ascii_lowercase();
        let is_web = matches!(porttype.as_str(), "http" | "https");
        Self {
            token: usertoken.to_owned(),
            tunnelname: plan.name.clone(),
            node: node.to_owned(),
            porttype,
            localip: plan.local_ip.clone(),
            localport: plan.local_port,
            remoteport: (!is_web).then_some(remote_port),
            encryption: false,
            compression: true,
            extraparams: None,
            banddomain: is_web.then(|| plan.domain.clone()),
        }
    }
}

#[derive(Debug, Serialize)]
struct TunnelUpdatePayload {
    tunnelid: i64,
    tunnelname: String,
    node: String,
    porttype: String,
    localip: String,
    localport: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    remoteport: Option<i64>,
    encryption: String,
    compression: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    extraparams: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    banddomain: Option<String>,
}

impl TunnelUpdatePayload {
    fn from_remote_input(tunnel_id: i64, input: &RemoteTunnelMutation) -> Self {
        let porttype = input.port_type.trim().to_ascii_lowercase();
        let is_web = matches!(porttype.as_str(), "http" | "https");
        Self {
            tunnelid: tunnel_id,
            tunnelname: input.tunnel_name.trim().to_owned(),
            node: input.node.trim().to_owned(),
            porttype,
            localip: input.local_ip.trim().to_owned(),
            localport: input.local_port,
            remoteport: (!is_web).then_some(input.remote_port),
            encryption: input.encryption.to_string(),
            compression: input.compression.to_string(),
            extraparams: (!input.extra_params.trim().is_empty())
                .then(|| input.extra_params.clone()),
            banddomain: is_web.then(|| input.domain.trim().to_owned()),
        }
    }

    fn from_plan_and_remote(remote: &RemoteTunnel, plan: &TunnelPlan, target_node: &str) -> Self {
        let porttype = plan.protocol.to_ascii_lowercase();
        let is_web = matches!(porttype.as_str(), "http" | "https");
        let extra = remote.extra_params.as_deref().unwrap_or_default().trim();
        Self {
            tunnelid: remote.tunnel_id,
            tunnelname: plan.name.clone(),
            node: target_node.to_owned(),
            porttype,
            localip: plan.local_ip.clone(),
            localport: plan.local_port,
            remoteport: (!is_web).then_some(remote.remote_port_number()),
            encryption: parse_boolish(&remote.encryption).to_string(),
            compression: parse_boolish(&remote.compression).to_string(),
            extraparams: (!extra.is_empty()).then(|| extra.to_owned()),
            banddomain: is_web.then(|| plan.domain.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteTunnel;

    #[test]
    fn parses_current_documented_list_shape() {
        let tunnel: RemoteTunnel = serde_json::from_value(serde_json::json!({
            "id": 17,
            "name": "web",
            "node": "node-a",
            "type": "http",
            "localip": "192.168.8.11",
            "nport": 3001,
            "dorp": "api.example.com",
            "encryption": "false",
            "compression": "true",
            "ap": "",
            "state": "true"
        }))
        .unwrap();
        assert_eq!(tunnel.tunnel_name, "web");
        assert_eq!(tunnel.local_port, 3001);
        assert_eq!(tunnel.effective_domain(), "api.example.com");
    }

    #[test]
    fn parses_legacy_camel_case_shape() {
        let tunnel: RemoteTunnel = serde_json::from_value(serde_json::json!({
            "tunnelID": 17,
            "tunnelName": "tcp-a",
            "node": "node-a",
            "portType": "tcp",
            "localIP": "127.0.0.1",
            "localPort": 8080,
            "remotePort": 12345,
            "encryption": false,
            "compression": true,
            "tunnelState": "false"
        }))
        .unwrap();
        assert_eq!(tunnel.remote_port_number(), 12345);
        assert_eq!(tunnel.encryption, "false");
    }
}
