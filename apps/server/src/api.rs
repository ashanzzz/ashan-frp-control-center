use crate::{
    coordinator::{Coordinator, ProviderSettingsApplyError},
    reconcile,
    state::AppState,
};
use ashan_frp_chmlfrp::{ChmlFrpClient, RemoteTunnel, RemoteTunnelMutation};
use ashan_frp_cloudflare::{CloudflareClient, CloudflareZone, DnsRecord, DnsRecordMutation};
use ashan_frp_domain::{
    ApiResponse, DashboardSnapshot, ErrorResponse, ProviderHealth, ProviderSettingsUpdate,
    ProviderSettingsView, RoutingUpdate, TunnelPlan, TunnelPlanInput,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    routing::{get, post, put},
};
use futures_util::Stream;
use serde::Deserialize;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

#[derive(Clone)]
struct ApiState {
    app: Arc<AppState>,
    coordinator: Coordinator,
}

pub fn router(app: Arc<AppState>, coordinator: Coordinator) -> Router {
    let state = ApiState { app, coordinator };
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/ready", get(ready))
        .route("/api/v1/dashboard", get(dashboard))
        .route("/api/v1/tunnels", get(list_tunnels).post(create_tunnel))
        .route(
            "/api/v1/tunnels/{id}",
            put(update_tunnel).delete(delete_tunnel),
        )
        .route("/api/v1/routing", get(routing).put(update_routing))
        .route(
            "/api/v1/settings/providers",
            get(provider_settings).put(update_provider_settings),
        )
        .route(
            "/api/v1/settings/providers/test/chmlfrp",
            post(test_chmlfrp),
        )
        .route(
            "/api/v1/settings/providers/test/cloudflare",
            post(test_cloudflare),
        )
        .route(
            "/api/v1/settings/providers/cloudflare/zones",
            post(cloudflare_zones),
        )
        .route(
            "/api/v1/chmlfrp/tunnels",
            get(chmlfrp_tunnels).post(chmlfrp_create_tunnel),
        )
        .route(
            "/api/v1/chmlfrp/tunnels/{id}",
            put(chmlfrp_update_tunnel).delete(chmlfrp_delete_tunnel),
        )
        .route(
            "/api/v1/chmlfrp/tunnels/{id}/test-write",
            post(chmlfrp_test_write),
        )
        .route("/api/v1/chmlfrp/diagnostics", post(chmlfrp_diagnostics))
        .route("/api/v1/tunnels/import", post(import_remote_tunnel))
        .route(
            "/api/v1/tunnels/import-all",
            post(import_all_remote_tunnels),
        )
        .route("/api/v1/tunnels/{id}/unmanage", post(unmanage_tunnel))
        .route("/api/v1/routing/bootstrap", post(bootstrap_routing))
        .route(
            "/api/v1/dns/records",
            get(dns_records).post(create_dns_record),
        )
        .route(
            "/api/v1/dns/records/{id}",
            put(update_dns_record).delete(delete_dns_record),
        )
        .route("/api/v1/dns/diagnostics", post(dns_diagnostics))
        .route("/api/v1/nodes", get(nodes))
        .route("/api/v1/nodes/{name}/unquarantine", post(unquarantine_node))
        .route("/api/v1/reconcile", post(reconcile_all))
        .route("/api/v1/failover", post(manual_failover))
        .route("/api/v1/frpc/status", get(frpc_status))
        .route("/api/v1/frpc/logs", get(frpc_logs))
        .route("/api/v1/frpc/start", post(frpc_start))
        .route("/api/v1/frpc/stop", post(frpc_stop))
        .route("/api/v1/frpc/restart", post(frpc_restart))
        .route("/api/v1/events", get(events))
        .with_state(state)
}

type ApiResult<T> = Result<Json<ApiResponse<T>>, ApiError>;

struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "BAD_REQUEST",
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "RESOURCE_CONFLICT",
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR",
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                code: self.code.into(),
                message: self.message,
            }),
        )
            .into_response()
    }
}

async fn health(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    Ok(Json(ApiResponse {
        data: serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "frpc": state.app.frpc.status().await.running,
        }),
    }))
}

async fn ready(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    state.app.db.ping().await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"status": "ready"}),
    }))
}

async fn dashboard(State(state): State<ApiState>) -> ApiResult<DashboardSnapshot> {
    let routing = state.app.db.routing_state().await?;
    let active = dashboard_node(&state, routing.active_node.as_deref()).await?;
    let standby = dashboard_node(&state, routing.standby_node.as_deref()).await?;

    Ok(Json(ApiResponse {
        data: DashboardSnapshot {
            routing,
            active_node: active,
            standby_node: standby,
            frpc: state.app.frpc.status().await,
            chmlfrp_health: provider_health(
                state.app.chml.configured(),
                state.app.chml.health().await,
            ),
            cloudflare_health: provider_health(
                state.app.cf.configured(),
                state.app.cf.health().await,
            ),
            tunnel_rows: reconcile::build_rows(&state.app).await?,
            recent_activity: state.app.db.recent_activity(50).await?,
            failover_job_id: state.app.failover_job.read().await.clone(),
        },
    }))
}

async fn dashboard_node(
    state: &ApiState,
    node_name: Option<&str>,
) -> Result<Option<ashan_frp_domain::NodeSummary>, ApiError> {
    let Some(node_name) = node_name else {
        return Ok(None);
    };
    let Ok(mut node) = state.app.chml.node_summary(node_name).await else {
        return Ok(None);
    };
    node.quarantined_until = state.app.db.quarantine_until(&node.name).await?;
    Ok(Some(node))
}

fn provider_health(configured: bool, result: anyhow::Result<()>) -> ProviderHealth {
    match result {
        Ok(()) => ProviderHealth {
            configured,
            connected: true,
            message: "已连接".into(),
        },
        Err(error) => ProviderHealth {
            configured,
            connected: false,
            message: error.to_string(),
        },
    }
}

async fn list_tunnels(State(state): State<ApiState>) -> ApiResult<Vec<TunnelPlan>> {
    Ok(Json(ApiResponse {
        data: state.app.db.list_tunnels().await?,
    }))
}

async fn create_tunnel(
    State(state): State<ApiState>,
    Json(input): Json<TunnelPlanInput>,
) -> ApiResult<TunnelPlan> {
    validate_tunnel(&input)?;
    Ok(Json(ApiResponse {
        data: state.app.db.create_tunnel(&input).await?,
    }))
}

async fn update_tunnel(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(input): Json<TunnelPlanInput>,
) -> ApiResult<TunnelPlan> {
    validate_tunnel(&input)?;
    Ok(Json(ApiResponse {
        data: state.app.db.update_tunnel(id, &input).await?,
    }))
}

async fn delete_tunnel(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let plan = state.app.db.get_tunnel(id).await?;
    if !state.app.chml.configured() {
        return Err(ApiError::conflict(
            "无法安全删除：ChmlFrp 未配置，无法确认远端隧道是否仍存在",
        ));
    }
    let remote = state.app.chml.list_tunnels().await?;
    if remote.iter().any(|tunnel| tunnel.tunnel_name == plan.name) {
        return Err(ApiError::conflict(format!(
            "无法删除计划：ChmlFrp 隧道 '{}' 仍存在，请先清理远端资源",
            plan.name
        )));
    }
    if plan.dns_managed {
        if !state.app.cf.configured() {
            return Err(ApiError::conflict(
                "无法安全删除：该计划管理 DNS，但 Cloudflare 未配置",
            ));
        }
        let records = state.app.cf.list_a_records().await?;
        if records
            .iter()
            .any(|record| record.name.eq_ignore_ascii_case(&plan.domain))
        {
            return Err(ApiError::conflict(format!(
                "无法删除计划：Cloudflare A 记录 '{}' 仍存在",
                plan.domain
            )));
        }
    }
    state.app.db.delete_tunnel(id).await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"deleted": id}),
    }))
}

fn validate_tunnel(input: &TunnelPlanInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() || input.local_ip.trim().is_empty() {
        return Err(ApiError::bad_request("name and local_ip are required"));
    }
    if !(1..=65535).contains(&input.local_port) {
        return Err(ApiError::bad_request("local_port must be 1..65535"));
    }
    let protocol = input.protocol.to_ascii_lowercase();
    if !matches!(protocol.as_str(), "http" | "https" | "tcp" | "udp") {
        return Err(ApiError::bad_request(
            "protocol must be http, https, tcp or udp",
        ));
    }
    if matches!(protocol.as_str(), "http" | "https") && input.domain.trim().is_empty() {
        return Err(ApiError::bad_request(
            "domain is required for HTTP/HTTPS tunnels",
        ));
    }
    if input.dns_managed && input.domain.trim().is_empty() {
        return Err(ApiError::bad_request(
            "domain is required when Cloudflare DNS is managed",
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ProviderProbeInput {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    zone_id: Option<String>,
}

async fn provider_settings(State(state): State<ApiState>) -> ApiResult<ProviderSettingsView> {
    Ok(Json(ApiResponse {
        data: state.app.db.provider_settings().await?.view(),
    }))
}

async fn update_provider_settings(
    State(state): State<ApiState>,
    Json(input): Json<ProviderSettingsUpdate>,
) -> ApiResult<ProviderSettingsView> {
    validate_api_base("ChmlFrp API Base", &input.chmlfrp_base_url)?;
    validate_api_base("Cloudflare API Base", &input.cloudflare_api_base)?;
    let view = match state.coordinator.apply_provider_settings(&input).await {
        Ok(view) => view,
        Err(ProviderSettingsApplyError::Busy) => {
            return Err(ApiError::conflict(
                "全局同步/故障切换正在运行，请在操作结束后再修改 Provider 配置",
            ));
        }
        Err(ProviderSettingsApplyError::Other(error)) => return Err(ApiError::from(error)),
    };
    Ok(Json(ApiResponse { data: view }))
}

async fn test_chmlfrp(
    State(state): State<ApiState>,
    Json(input): Json<ProviderProbeInput>,
) -> ApiResult<serde_json::Value> {
    let saved = state.app.db.provider_settings().await?;
    let base_url = candidate_value(input.base_url.as_deref(), &saved.chmlfrp_base_url);
    let token = candidate_value(input.token.as_deref(), &saved.chmlfrp_token);
    validate_api_base("ChmlFrp API Base", &base_url)?;
    if token.is_empty() {
        return Err(ApiError::bad_request("请输入 ChmlFrp API Token"));
    }

    let client = ChmlFrpClient::new(&base_url, &token)?;
    let tunnels = client.list_tunnels().await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({
            "ok": true,
            "message": "ChmlFrp API 连接成功，Token 有效",
            "tunnels": tunnels.len(),
        }),
    }))
}

async fn test_cloudflare(
    State(state): State<ApiState>,
    Json(input): Json<ProviderProbeInput>,
) -> ApiResult<serde_json::Value> {
    let saved = state.app.db.provider_settings().await?;
    let base_url = candidate_value(input.base_url.as_deref(), &saved.cloudflare_api_base);
    let token = candidate_value(input.token.as_deref(), &saved.cloudflare_api_token);
    let zone_id = candidate_value(input.zone_id.as_deref(), &saved.cloudflare_zone_id);
    validate_api_base("Cloudflare API Base", &base_url)?;
    if token.is_empty() {
        return Err(ApiError::bad_request("请输入 Cloudflare API Token"));
    }

    let client = CloudflareClient::new(&base_url, &token, &zone_id)?;
    let token_status = client.verify_token().await?;
    if zone_id.is_empty() {
        let zones = client.list_zones().await?;
        return Ok(Json(ApiResponse {
            data: serde_json::json!({
                "ok": true,
                "message": "Cloudflare Token 有效；请选择 Zone 后再测试 DNS 读取",
                "token_status": token_status,
                "zones": zones.len(),
                "dns_read_tested": false,
            }),
        }));
    }

    let records = client.list_a_records().await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({
            "ok": true,
            "message": "Cloudflare Token 与 Zone 读取测试成功",
            "token_status": token_status,
            "a_records": records.len(),
            "dns_read_tested": true,
            "dns_write_tested": false,
        }),
    }))
}

async fn cloudflare_zones(
    State(state): State<ApiState>,
    Json(input): Json<ProviderProbeInput>,
) -> ApiResult<Vec<CloudflareZone>> {
    let saved = state.app.db.provider_settings().await?;
    let base_url = candidate_value(input.base_url.as_deref(), &saved.cloudflare_api_base);
    let token = candidate_value(input.token.as_deref(), &saved.cloudflare_api_token);
    validate_api_base("Cloudflare API Base", &base_url)?;
    if token.is_empty() {
        return Err(ApiError::bad_request("请输入 Cloudflare API Token"));
    }

    let client = CloudflareClient::new(&base_url, &token, "")?;
    client.verify_token().await?;
    let mut zones = client.list_zones().await?;
    zones.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(ApiResponse { data: zones }))
}

fn candidate_value(candidate: Option<&str>, current: &str) -> String {
    candidate
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(current)
        .trim()
        .to_owned()
}

fn validate_api_base(label: &str, value: &str) -> Result<(), ApiError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ApiError::bad_request(format!("{label} 不能为空")));
    }
    let secure = value.starts_with("https://");
    let loopback = value == "http://127.0.0.1"
        || value.starts_with("http://127.0.0.1:")
        || value.starts_with("http://127.0.0.1/")
        || value == "http://localhost"
        || value.starts_with("http://localhost:")
        || value.starts_with("http://localhost/")
        || value == "http://[::1]"
        || value.starts_with("http://[::1]:")
        || value.starts_with("http://[::1]/");
    if !secure && !loopback {
        return Err(ApiError::bad_request(format!(
            "{label} 必须使用 https://；仅本机回环地址允许 http://"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct TunnelImportInput {
    tunnel_id: i64,
    #[serde(default)]
    dns_managed: bool,
}

#[derive(Debug, Deserialize)]
struct RoutingBootstrapInput {
    active_node: String,
    #[serde(default)]
    standby_node: Option<String>,
    #[serde(default = "default_quarantine_days")]
    quarantine_days: i64,
    #[serde(default = "default_true")]
    failover_enabled: bool,
}

fn default_quarantine_days() -> i64 {
    30
}

fn default_true() -> bool {
    true
}

async fn chmlfrp_tunnels(State(state): State<ApiState>) -> ApiResult<Vec<RemoteTunnel>> {
    Ok(Json(ApiResponse {
        data: state.app.chml.list_tunnels().await?,
    }))
}

fn validate_remote_tunnel(input: &RemoteTunnelMutation) -> Result<(), ApiError> {
    if input.tunnel_name.trim().is_empty()
        || input.node.trim().is_empty()
        || input.local_ip.trim().is_empty()
    {
        return Err(ApiError::bad_request(
            "tunnel_name, node and local_ip are required",
        ));
    }
    if !(1..=65535).contains(&input.local_port) {
        return Err(ApiError::bad_request("local_port must be 1..65535"));
    }
    let protocol = input.port_type.to_ascii_lowercase();
    if !matches!(protocol.as_str(), "http" | "https" | "tcp" | "udp") {
        return Err(ApiError::bad_request(
            "port_type must be http, https, tcp or udp",
        ));
    }
    if matches!(protocol.as_str(), "http" | "https") && input.domain.trim().is_empty() {
        return Err(ApiError::bad_request(
            "domain is required for HTTP/HTTPS tunnels",
        ));
    }
    if matches!(protocol.as_str(), "tcp" | "udp") && !(1..=65535).contains(&input.remote_port) {
        return Err(ApiError::bad_request(
            "remote_port must be 1..65535 for TCP/UDP tunnels",
        ));
    }
    Ok(())
}

async fn chmlfrp_create_tunnel(
    State(state): State<ApiState>,
    Json(input): Json<RemoteTunnelMutation>,
) -> ApiResult<RemoteTunnel> {
    validate_remote_tunnel(&input)?;
    if state
        .app
        .db
        .list_tunnels()
        .await?
        .iter()
        .any(|plan| plan.name == input.tunnel_name)
    {
        return Err(ApiError::conflict(
            "同名隧道已由 Ashan 管理；请在计划隧道中修改后执行全局同步",
        ));
    }
    let created = state.app.chml.create_remote_tunnel(&input).await?;
    state
        .app
        .db
        .activity(
            None,
            "chmlfrp_crud",
            "info",
            "创建 ChmlFrp 远端隧道",
            Some(&created.tunnel_name),
            Some(&created.node),
            serde_json::json!({"tunnel_id": created.tunnel_id}),
        )
        .await?;
    Ok(Json(ApiResponse { data: created }))
}

async fn chmlfrp_update_tunnel(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(input): Json<RemoteTunnelMutation>,
) -> ApiResult<serde_json::Value> {
    validate_remote_tunnel(&input)?;
    let remote = state.app.chml.list_tunnels().await?;
    let current = remote
        .iter()
        .find(|tunnel| tunnel.tunnel_id == id)
        .ok_or_else(|| ApiError::bad_request("ChmlFrp tunnel not found"))?;
    if current.tunnel_name != input.tunnel_name {
        return Err(ApiError::bad_request(
            "直接远端编辑不支持重命名隧道；请保持原隧道名",
        ));
    }
    if state
        .app
        .db
        .list_tunnels()
        .await?
        .iter()
        .any(|plan| plan.name == current.tunnel_name)
    {
        return Err(ApiError::conflict(
            "该隧道已由 Ashan 管理；受管隧道必须修改计划后执行 GLOBAL RECONCILE，不能直接改远端",
        ));
    }
    state.app.chml.update_remote_tunnel(&input).await?;
    state
        .app
        .db
        .activity(
            None,
            "chmlfrp_crud",
            "warning",
            "修改未纳管 ChmlFrp 远端隧道",
            Some(&input.tunnel_name),
            Some(&input.node),
            serde_json::json!({"tunnel_id": id}),
        )
        .await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"updated": id}),
    }))
}

fn remote_boolish(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn mutation_from_remote(tunnel: &RemoteTunnel) -> Result<RemoteTunnelMutation, ApiError> {
    let protocol = tunnel.port_type.trim().to_ascii_lowercase();
    let remote_port = if matches!(protocol.as_str(), "tcp" | "udp") {
        tunnel.remote_endpoint.parse::<i64>().map_err(|_| {
            ApiError::bad_request("当前 TCP/UDP 远端端口无法解析，不能执行安全写入测试")
        })?
    } else {
        0
    };
    Ok(RemoteTunnelMutation {
        tunnel_name: tunnel.tunnel_name.clone(),
        node: tunnel.node.clone(),
        port_type: protocol,
        local_ip: tunnel.local_ip.clone(),
        local_port: tunnel.local_port,
        remote_port,
        domain: tunnel.effective_domain(),
        encryption: remote_boolish(&tunnel.encryption),
        compression: remote_boolish(&tunnel.compression),
        extra_params: tunnel.extra_params.clone().unwrap_or_default(),
    })
}

async fn chmlfrp_test_write(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let remote = state.app.chml.list_tunnels().await?;
    let current = remote
        .iter()
        .find(|tunnel| tunnel.tunnel_id == id)
        .cloned()
        .ok_or_else(|| ApiError::bad_request("ChmlFrp tunnel not found"))?;
    if state
        .app
        .db
        .list_tunnels()
        .await?
        .iter()
        .any(|plan| plan.name == current.tunnel_name)
    {
        return Err(ApiError::conflict(
            "受管隧道禁止直接写测试；请仅对 Unmanaged 远端资源执行测试",
        ));
    }

    let mutation = mutation_from_remote(&current)?;
    state.app.chml.update_remote_tunnel(&mutation).await?;

    let after = state
        .app
        .chml
        .list_tunnels()
        .await?
        .into_iter()
        .find(|tunnel| tunnel.tunnel_id == id)
        .ok_or_else(|| ApiError::conflict("写入后未能重新读取到测试隧道"))?;
    let after_mutation = mutation_from_remote(&after)?;
    let preserved = after_mutation.tunnel_name == mutation.tunnel_name
        && after_mutation.node == mutation.node
        && after_mutation.port_type == mutation.port_type
        && after_mutation.local_ip == mutation.local_ip
        && after_mutation.local_port == mutation.local_port
        && after_mutation.remote_port == mutation.remote_port
        && after_mutation.domain.eq_ignore_ascii_case(&mutation.domain)
        && after_mutation.encryption == mutation.encryption
        && after_mutation.compression == mutation.compression
        && after_mutation.extra_params == mutation.extra_params;
    if !preserved {
        return Err(ApiError::conflict(
            "ChmlFrp 写 API 返回成功，但重新读取后的配置与写入前不一致；请在官方面板核对该隧道",
        ));
    }

    state
        .app
        .db
        .activity(
            None,
            "chmlfrp_diagnostics",
            "info",
            "完成未纳管 ChmlFrp 隧道原值写回测试；重新读取后配置一致",
            Some(&current.tunnel_name),
            Some(&current.node),
            serde_json::json!({"tunnel_id": id, "preserved": true}),
        )
        .await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({
            "ok": true,
            "tunnel_id": id,
            "tunnel": current.tunnel_name,
            "node": current.node,
            "preserved": true,
            "message": "写 API 可用；原值写回后重新读取一致",
        }),
    }))
}

async fn chmlfrp_delete_tunnel(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let _ = state;
    Err(ApiError::conflict(format!(
        "ChmlFrp v2 删除接口当前官方标记为不可用，因此控制中心不会调用未稳定删除接口；tunnel_id={id}。请先在官方面板删除，随后刷新本页。"
    )))
}

async fn chmlfrp_diagnostics(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    let tunnels = state.app.chml.list_tunnels().await?;
    let nodes = state.app.chml.list_nodes().await?;
    let mut config_test = serde_json::json!({"tested": false});
    if let Some(first) = tunnels.first() {
        let names = tunnels
            .iter()
            .filter(|tunnel| tunnel.node == first.node)
            .map(|tunnel| tunnel.tunnel_name.clone())
            .collect::<Vec<_>>();
        if !names.is_empty() {
            match state.app.chml.generated_config(&first.node, &names).await {
                Ok(config) => {
                    config_test = serde_json::json!({
                        "tested": true,
                        "ok": true,
                        "node": first.node,
                        "tunnels": names.len(),
                        "bytes": config.len(),
                    });
                }
                Err(error) => {
                    config_test = serde_json::json!({
                        "tested": true,
                        "ok": false,
                        "node": first.node,
                        "message": error.to_string(),
                    });
                }
            }
        }
    }
    Ok(Json(ApiResponse {
        data: serde_json::json!({
            "authentication": "pass",
            "tunnel_read": {"ok": true, "count": tunnels.len()},
            "node_read": {"ok": true, "count": nodes.len()},
            "config_generate": config_test,
            "delete_supported": state.app.chml.delete_tunnel_supported(),
        }),
    }))
}

async fn import_remote_tunnel(
    State(state): State<ApiState>,
    Json(input): Json<TunnelImportInput>,
) -> ApiResult<TunnelPlan> {
    let remote = state.app.chml.list_tunnels().await?;
    let tunnel = remote
        .iter()
        .find(|tunnel| tunnel.tunnel_id == input.tunnel_id)
        .ok_or_else(|| ApiError::bad_request("ChmlFrp tunnel not found"))?;
    if state
        .app
        .db
        .list_tunnels()
        .await?
        .iter()
        .any(|plan| plan.name == tunnel.tunnel_name)
    {
        return Err(ApiError::conflict("该 ChmlFrp 隧道已经纳管"));
    }
    let domain = tunnel.effective_domain();
    if input.dns_managed && domain.trim().is_empty() {
        return Err(ApiError::bad_request(
            "该隧道没有可用于 Cloudflare A 记录的域名，不能直接启用 DNS 纳管",
        ));
    }
    let plan_input = TunnelPlanInput {
        name: tunnel.tunnel_name.clone(),
        local_ip: tunnel.local_ip.clone(),
        local_port: tunnel.local_port,
        protocol: tunnel.port_type.to_ascii_lowercase(),
        domain,
        dns_managed: input.dns_managed,
    };
    validate_tunnel(&plan_input)?;
    let plan = state.app.db.create_tunnel(&plan_input).await?;
    state
        .app
        .db
        .activity(
            None,
            "import",
            "info",
            "导入 ChmlFrp 现有隧道为 Ashan 计划；导入本身未修改远端",
            Some(&plan.name),
            Some(&tunnel.node),
            serde_json::json!({"remote_tunnel_id": tunnel.tunnel_id, "dns_managed": input.dns_managed}),
        )
        .await?;
    Ok(Json(ApiResponse { data: plan }))
}

async fn unmanage_tunnel(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let plan = state.app.db.get_tunnel(id).await?;
    state.app.db.delete_tunnel(id).await?;
    state
        .app
        .db
        .activity(
            None,
            "unmanage",
            "warning",
            "解除 Ashan 纳管；ChmlFrp 远端隧道与 Cloudflare DNS 均保持不变",
            Some(&plan.name),
            None,
            serde_json::json!({"plan_id": id, "dns_was_managed": plan.dns_managed}),
        )
        .await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"unmanaged": id, "name": plan.name}),
    }))
}

async fn import_all_remote_tunnels(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    let remote = state.app.chml.list_tunnels().await?;
    let existing = state.app.db.list_tunnels().await?;
    let mut imported = Vec::new();
    let mut skipped = Vec::new();
    for tunnel in remote {
        if existing.iter().any(|plan| plan.name == tunnel.tunnel_name) {
            skipped.push(tunnel.tunnel_name);
            continue;
        }
        let input = TunnelPlanInput {
            name: tunnel.tunnel_name.clone(),
            local_ip: tunnel.local_ip.clone(),
            local_port: tunnel.local_port,
            protocol: tunnel.port_type.to_ascii_lowercase(),
            domain: tunnel.effective_domain(),
            dns_managed: false,
        };
        if validate_tunnel(&input).is_err() {
            skipped.push(tunnel.tunnel_name);
            continue;
        }
        match state.app.db.create_tunnel(&input).await {
            Ok(plan) => imported.push(plan.name),
            Err(_) => skipped.push(tunnel.tunnel_name),
        }
    }
    state
        .app
        .db
        .activity(
            None,
            "import",
            "info",
            "批量导入 ChmlFrp 现有隧道；默认不自动纳管 DNS",
            None,
            None,
            serde_json::json!({"imported": imported.clone(), "skipped": skipped.clone()}),
        )
        .await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"imported": imported, "skipped": skipped}),
    }))
}

async fn bootstrap_routing(
    State(state): State<ApiState>,
    Json(input): Json<RoutingBootstrapInput>,
) -> ApiResult<ashan_frp_domain::RoutingState> {
    let current = state.app.db.routing_state().await?;
    if current.active_node.is_some() {
        return Err(ApiError::conflict(
            "全局路由已经初始化；ACTIVE 只能通过 GLOBAL_FAILOVER 迁移",
        ));
    }
    if !(1..=3650).contains(&input.quarantine_days) {
        return Err(ApiError::bad_request("quarantine_days must be 1..3650"));
    }
    let active = input.active_node.trim();
    if active.is_empty() {
        return Err(ApiError::bad_request("active_node is required"));
    }
    let standby = input
        .standby_node
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if standby == Some(active) {
        return Err(ApiError::bad_request(
            "active_node and standby_node must differ",
        ));
    }
    let active_info = state.app.chml.node_info(active).await?;
    if !active_info.state.eq_ignore_ascii_case("online") {
        return Err(ApiError::conflict(format!(
            "ACTIVE 节点当前不是 online：{}",
            active_info.state
        )));
    }
    if let Some(standby_name) = standby {
        let standby_info = state.app.chml.node_info(standby_name).await?;
        if !standby_info.state.eq_ignore_ascii_case("online") {
            return Err(ApiError::conflict(format!(
                "STANDBY 节点当前不是 online：{}",
                standby_info.state
            )));
        }
    }
    let routing = state
        .app
        .db
        .update_routing(&RoutingUpdate {
            active_node: Some(active.to_owned()),
            standby_node: standby.map(str::to_owned),
            quarantine_days: input.quarantine_days,
            failover_enabled: input.failover_enabled,
        })
        .await?;
    state
        .app
        .db
        .activity(
            None,
            "routing_bootstrap",
            "warning",
            "初始化全局 ACTIVE / STANDBY；尚未自动迁移隧道，需执行全局同步",
            None,
            Some(active),
            serde_json::json!({"standby": standby}),
        )
        .await?;
    Ok(Json(ApiResponse { data: routing }))
}

async fn dns_records(State(state): State<ApiState>) -> ApiResult<Vec<DnsRecord>> {
    Ok(Json(ApiResponse {
        data: state.app.cf.list_dns_records().await?,
    }))
}

fn validate_dns_record(input: &DnsRecordMutation) -> Result<(), ApiError> {
    let record_type = input.record_type.trim().to_ascii_uppercase();
    if !matches!(record_type.as_str(), "A" | "AAAA" | "CNAME" | "TXT" | "MX") {
        return Err(ApiError::bad_request(
            "DNS type must be A, AAAA, CNAME, TXT or MX",
        ));
    }
    if input.name.trim().is_empty() || input.content.trim().is_empty() {
        return Err(ApiError::bad_request("DNS name and content are required"));
    }
    if input.ttl != 1 && !(60..=86400).contains(&input.ttl) {
        return Err(ApiError::bad_request(
            "TTL must be 1 (Auto) or between 60 and 86400 seconds",
        ));
    }
    if record_type == "MX" && input.priority.is_none() {
        return Err(ApiError::bad_request("MX record requires priority"));
    }
    Ok(())
}

async fn managed_plan_for_dns(
    state: &ApiState,
    record_id: &str,
    name: &str,
) -> Result<Option<TunnelPlan>, ApiError> {
    Ok(state.app.db.list_tunnels().await?.into_iter().find(|plan| {
        plan.dns_managed
            && (plan.cloudflare_record_id.as_deref() == Some(record_id)
                || plan.domain.eq_ignore_ascii_case(name))
    }))
}

async fn create_dns_record(
    State(state): State<ApiState>,
    Json(input): Json<DnsRecordMutation>,
) -> ApiResult<DnsRecord> {
    validate_dns_record(&input)?;
    if state
        .app
        .db
        .list_tunnels()
        .await?
        .iter()
        .any(|plan| plan.dns_managed && plan.domain.eq_ignore_ascii_case(input.name.trim()))
    {
        return Err(ApiError::conflict(
            "该域名已由 Ashan GLOBAL HA 管理；请通过计划隧道和全局同步维护 A 记录",
        ));
    }
    let created = state.app.cf.create_dns_record(&input).await?;
    state
        .app
        .db
        .activity(
            None,
            "dns_crud",
            "info",
            "创建 Cloudflare DNS 记录",
            None,
            None,
            serde_json::json!({"record": created.clone()}),
        )
        .await?;
    Ok(Json(ApiResponse { data: created }))
}

async fn update_dns_record(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(input): Json<DnsRecordMutation>,
) -> ApiResult<DnsRecord> {
    validate_dns_record(&input)?;
    let current = state.app.cf.get_dns_record(&id).await?;
    if let Some(plan) = managed_plan_for_dns(&state, &id, &current.name).await? {
        return Err(ApiError::conflict(format!(
            "DNS 记录 '{}' 由隧道 '{}' 的 GLOBAL HA 管理；请先在计划隧道中解除 DNS 纳管",
            current.name, plan.name
        )));
    }
    let updated = state.app.cf.update_dns_record(&id, &input).await?;
    state
        .app
        .db
        .activity(
            None,
            "dns_crud",
            "warning",
            "修改 Cloudflare DNS 记录",
            None,
            None,
            serde_json::json!({"before": current.clone(), "after": updated.clone()}),
        )
        .await?;
    Ok(Json(ApiResponse { data: updated }))
}

async fn delete_dns_record(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let current = state.app.cf.get_dns_record(&id).await?;
    if let Some(plan) = managed_plan_for_dns(&state, &id, &current.name).await? {
        return Err(ApiError::conflict(format!(
            "DNS 记录 '{}' 由隧道 '{}' 的 GLOBAL HA 管理；请先解除 DNS 纳管",
            current.name, plan.name
        )));
    }
    let deleted_id = state.app.cf.delete_dns_record(&id).await?;
    state
        .app
        .db
        .activity(
            None,
            "dns_crud",
            "warning",
            "删除 Cloudflare DNS 记录；删除前快照已记录",
            None,
            None,
            serde_json::json!({"deleted": current.clone()}),
        )
        .await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"deleted": deleted_id, "snapshot": current}),
    }))
}

async fn dns_diagnostics(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    let token_status = state.app.cf.verify_token().await?;
    let settings = state.app.db.provider_settings().await?;
    if settings.cloudflare_zone_id.trim().is_empty() {
        return Err(ApiError::bad_request(
            "Cloudflare CRUD 测试需要先选择并保存 Zone",
        ));
    }
    let zones = state.app.cf.list_zones().await?;
    let zone = zones
        .iter()
        .find(|zone| zone.id == settings.cloudflare_zone_id)
        .ok_or_else(|| ApiError::bad_request("当前 Zone ID 不在 Token 可访问 Zone 中"))?;
    let before_count = state.app.cf.list_dns_records().await?.len();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let test_name = format!("_ashan-api-test-{}.{}", &suffix[..8], zone.name);
    let create_input = DnsRecordMutation {
        record_type: "TXT".to_owned(),
        name: test_name.clone(),
        content: "ashan-api-test".to_owned(),
        ttl: 60,
        proxied: None,
        priority: None,
        comment: Some("Temporary Ashan FRP API CRUD test".to_owned()),
    };
    let created = state.app.cf.create_dns_record(&create_input).await?;
    let read_back = match state.app.cf.get_dns_record(&created.id).await {
        Ok(record) => record,
        Err(error) => {
            let _ = state.app.cf.delete_dns_record(&created.id).await;
            return Err(error.into());
        }
    };
    let update_input = DnsRecordMutation {
        content: "ashan-api-test-updated".to_owned(),
        ..create_input
    };
    let updated = match state
        .app
        .cf
        .update_dns_record(&created.id, &update_input)
        .await
    {
        Ok(record) => record,
        Err(error) => {
            let _ = state.app.cf.delete_dns_record(&created.id).await;
            return Err(error.into());
        }
    };
    state.app.cf.delete_dns_record(&created.id).await?;
    let deleted_absent = !state
        .app
        .cf
        .list_dns_records()
        .await?
        .iter()
        .any(|record| record.id == created.id);
    state
        .app
        .db
        .activity(
            None,
            "dns_diagnostics",
            "info",
            "完成 Cloudflare 临时 TXT 记录 CRUD 测试并清理测试记录",
            None,
            None,
            serde_json::json!({"test_name": test_name, "deleted_absent": deleted_absent}),
        )
        .await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({
            "authentication": token_status,
            "zone": zone.name,
            "read": {"ok": true, "records_before": before_count},
            "create": {"ok": true, "id": created.id},
            "read_back": {"ok": read_back.id == created.id},
            "update": {"ok": updated.content == "ashan-api-test-updated"},
            "delete": {"ok": deleted_absent},
            "cleanup": {"ok": deleted_absent},
        }),
    }))
}

async fn routing(State(state): State<ApiState>) -> ApiResult<ashan_frp_domain::RoutingState> {
    Ok(Json(ApiResponse {
        data: state.app.db.routing_state().await?,
    }))
}

async fn update_routing(
    State(state): State<ApiState>,
    Json(input): Json<RoutingUpdate>,
) -> ApiResult<ashan_frp_domain::RoutingState> {
    if !(1..=3650).contains(&input.quarantine_days) {
        return Err(ApiError::bad_request("quarantine_days must be 1..3650"));
    }
    if input.active_node.is_some() && input.active_node == input.standby_node {
        return Err(ApiError::bad_request(
            "active_node and standby_node must differ",
        ));
    }
    let current = state.app.db.routing_state().await?;
    if current.active_node.is_some() && current.active_node != input.active_node {
        return Err(ApiError::conflict(
            "active_node cannot be changed directly; use GLOBAL_FAILOVER",
        ));
    }
    Ok(Json(ApiResponse {
        data: state.app.db.update_routing(&input).await?,
    }))
}

async fn nodes(State(state): State<ApiState>) -> ApiResult<Vec<ashan_frp_domain::NodeSummary>> {
    let mut nodes = state.app.chml.list_nodes().await?;
    for node in &mut nodes {
        node.quarantined_until = state.app.db.quarantine_until(&node.name).await?;
    }
    Ok(Json(ApiResponse { data: nodes }))
}

async fn unquarantine_node(
    State(state): State<ApiState>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("node name is required"));
    }
    state.app.db.clear_quarantine(&name).await?;
    state
        .app
        .db
        .activity(
            None,
            "quarantine",
            "warning",
            "人工解除节点隔离；节点仅重新进入候选池，不触发自动回切",
            None,
            Some(&name),
            serde_json::json!({"manual_clear": true}),
        )
        .await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"node": name, "quarantined": false}),
    }))
}

async fn reconcile_all(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    let job = state.coordinator.reconcile().await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"job_id": job}),
    }))
}

async fn manual_failover(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    let job = state.coordinator.global_failover(None).await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"job_id": job}),
    }))
}

async fn frpc_status(
    State(state): State<ApiState>,
) -> ApiResult<ashan_frp_domain::FrpcRuntimeStatus> {
    Ok(Json(ApiResponse {
        data: state.app.frpc.status().await,
    }))
}

async fn frpc_logs(State(state): State<ApiState>) -> ApiResult<Vec<ashan_frp_domain::FrpcEvent>> {
    Ok(Json(ApiResponse {
        data: state.app.frpc.recent_logs(500).await,
    }))
}

async fn frpc_start(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    state.app.frpc.start().await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"ok": true}),
    }))
}

async fn frpc_stop(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    state.app.frpc.stop().await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"ok": true}),
    }))
}

async fn frpc_restart(State(state): State<ApiState>) -> ApiResult<serde_json::Value> {
    state.app.frpc.restart().await?;
    Ok(Json(ApiResponse {
        data: serde_json::json!({"ok": true}),
    }))
}

async fn events(
    State(state): State<ApiState>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let stream =
        BroadcastStream::new(state.app.frpc.subscribe()).filter_map(|result| match result {
            Ok(event) => Some(Ok(axum::response::sse::Event::default()
                .event("frpc")
                .json_data(event)
                .unwrap_or_else(|_| axum::response::sse::Event::default().data("{}")))),
            Err(_) => None,
        });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}
