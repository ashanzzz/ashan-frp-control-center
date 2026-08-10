use anyhow::{Context, Result, anyhow};
use ashan_frp_domain::{NodeSummary, TunnelPlan};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct ChmlFrpClient {
    http: Client,
    base_url: String,
    token: String,
}

impl ChmlFrpClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("build ChmlFrp HTTP client")?;

        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            token: token.into(),
        })
    }

    pub fn configured(&self) -> bool {
        !self.token.trim().is_empty()
    }

    pub async fn list_tunnels(&self) -> Result<Vec<RemoteTunnel>> {
        self.require_token()?;
        let response: Envelope<Vec<RemoteTunnel>> = self
            .http
            .get(format!("{}/tunnel", self.base_url))
            .query(&[("token", self.token.as_str())])
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

    pub async fn list_nodes(&self) -> Result<Vec<NodeSummary>> {
        let response: Envelope<Vec<NodeListItem>> = self
            .http
            .get(format!("{}/node", self.base_url))
            .send()
            .await
            .context("list ChmlFrp nodes")?
            .error_for_status()
            .context("ChmlFrp node-list HTTP status")?
            .json()
            .await
            .context("decode ChmlFrp node-list response")?;
        ensure_success(&response.state, response.code, &response.msg)?;

        let mut nodes = Vec::new();
        for item in response.data.unwrap_or_default() {
            let detail = if self.configured() {
                self.node_info(&item.name).await.ok()
            } else {
                None
            };
            nodes.push(NodeSummary {
                id: item.id,
                name: item.name,
                area: item.area,
                web_supported: item.web.eq_ignore_ascii_case("yes"),
                state: detail
                    .as_ref()
                    .map(|value| value.state.clone())
                    .unwrap_or_else(|| "unknown".to_owned()),
                real_ip: detail.as_ref().map(|value| value.real_ip.clone()),
                host: detail.as_ref().map(|value| value.ip.clone()),
                load1: detail.as_ref().map(|value| value.load1),
                bandwidth_usage_percent: detail
                    .as_ref()
                    .map(|value| value.bandwidth_usage_percent),
                quarantined_until: None,
            });
        }
        Ok(nodes)
    }

    pub async fn node_summary(&self, node: &str) -> Result<NodeSummary> {
        let info = self.node_info(node).await?;
        let name = nonempty(info.name).unwrap_or_else(|| node.to_owned());
        let real_ip = nonempty(info.real_ip);
        let host = nonempty(info.ip);
        Ok(NodeSummary {
            id: info.id,
            name,
            area: info.area,
            web_supported: info.web.eq_ignore_ascii_case("yes"),
            state: info.state,
            real_ip,
            host,
            load1: Some(info.load1),
            bandwidth_usage_percent: Some(info.bandwidth_usage_percent),
            quarantined_until: None,
        })
    }

    pub async fn node_info(&self, node: &str) -> Result<NodeInfo> {
        self.require_token()?;
        let response: Envelope<NodeInfo> = self
            .http
            .get(format!("{}/nodeinfo", self.base_url))
            .query(&[("token", self.token.as_str()), ("node", node)])
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

    pub async fn create_tunnel(
        &self,
        plan: &TunnelPlan,
        target_node: &str,
    ) -> Result<RemoteTunnel> {
        self.require_token()?;
        let payload = TunnelMutation::from_plan(&self.token, plan, target_node, 0);
        let response: Envelope<RemoteTunnel> = self
            .http
            .post(format!("{}/create_tunnel", self.base_url))
            .json(&payload)
            .send()
            .await
            .with_context(|| format!("create ChmlFrp tunnel {}", plan.name))?
            .error_for_status()
            .with_context(|| format!("ChmlFrp create_tunnel HTTP status for {}", plan.name))?
            .json()
            .await
            .with_context(|| format!("decode ChmlFrp create_tunnel response for {}", plan.name))?;
        ensure_success(&response.state, response.code, &response.msg)?;
        response
            .data
            .ok_or_else(|| anyhow!("ChmlFrp create_tunnel returned no data"))
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
        self.require_token()?;
        let is_web = matches!(
            plan.protocol.to_ascii_lowercase().as_str(),
            "http" | "https"
        );
        let payload = TunnelMutation {
            token: self.token.clone(),
            tunnelname: plan.name.clone(),
            node: target_node.to_owned(),
            porttype: plan.protocol.to_ascii_lowercase(),
            localip: plan.local_ip.clone(),
            localport: plan.local_port,
            remoteport: if is_web {
                0
            } else {
                remote.remote_port_number()
            },
            encryption: parse_boolish(&remote.encryption),
            compression: parse_boolish(&remote.compression),
            extraparams: remote.extra_params.clone().unwrap_or_default(),
            banddomain: if is_web {
                plan.domain.clone()
            } else {
                remote.effective_domain()
            },
        };
        let response: Envelope<serde_json::Value> = self
            .http
            .post(format!("{}/update_tunnel", self.base_url))
            .form(&payload)
            .send()
            .await
            .with_context(|| format!("sync ChmlFrp tunnel {}", plan.name))?
            .error_for_status()
            .with_context(|| format!("ChmlFrp update_tunnel HTTP status for {}", plan.name))?
            .json()
            .await
            .with_context(|| format!("decode ChmlFrp update_tunnel response for {}", plan.name))?;
        ensure_success(&response.state, response.code, &response.msg)
            .with_context(|| format!("sync tunnel {} to global node {target_node}", plan.name))
    }

    pub async fn generated_config(&self, node: &str, tunnel_names: &[String]) -> Result<String> {
        self.require_token()?;
        let names = tunnel_names.join(",");
        let response: Envelope<String> = self
            .http
            .get(format!("{}/tunnel_config", self.base_url))
            .query(&[
                ("token", self.token.as_str()),
                ("node", node),
                ("tunnel_names", names.as_str()),
            ])
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

    pub async fn health(&self) -> Result<()> {
        self.require_token()?;
        self.list_tunnels().await.map(|_| ())
    }

    fn require_token(&self) -> Result<()> {
        if self.configured() {
            Ok(())
        } else {
            Err(anyhow!("CHMLFRP_TOKEN is not configured"))
        }
    }
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
struct NodeListItem {
    id: i64,
    name: String,
    area: String,
    web: String,
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
    pub load1: f64,
    #[serde(default)]
    pub bandwidth_usage_percent: f64,
    #[serde(default)]
    pub web: String,
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
            && (!is_web
                || self
                    .effective_domain()
                    .eq_ignore_ascii_case(&plan.domain))
    }
}

fn deserialize_string_or_number<'de, D>(
    deserializer: D,
) -> std::result::Result<String, D::Error>
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
struct TunnelMutation {
    token: String,
    tunnelname: String,
    node: String,
    porttype: String,
    localip: String,
    localport: i64,
    remoteport: i64,
    encryption: bool,
    compression: bool,
    extraparams: String,
    banddomain: String,
}

impl TunnelMutation {
    fn from_plan(token: &str, plan: &TunnelPlan, node: &str, remote_port: i64) -> Self {
        Self {
            token: token.to_owned(),
            tunnelname: plan.name.clone(),
            node: node.to_owned(),
            porttype: plan.protocol.to_ascii_lowercase(),
            localip: plan.local_ip.clone(),
            localport: plan.local_port,
            remoteport: remote_port,
            encryption: false,
            compression: true,
            extraparams: String::new(),
            banddomain: plan.domain.clone(),
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
