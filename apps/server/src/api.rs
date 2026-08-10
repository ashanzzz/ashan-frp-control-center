use crate::{coordinator::Coordinator, reconcile, state::AppState};
use ashan_frp_domain::{
    ApiResponse, DashboardSnapshot, ErrorResponse, ProviderHealth, RoutingUpdate, TunnelPlan,
    TunnelPlanInput,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    routing::{get, post, put},
};
use futures_util::Stream;
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
        .route("/api/v1/nodes", get(nodes))
        .route(
            "/api/v1/nodes/{name}/unquarantine",
            post(unquarantine_node),
        )
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
    let stream = BroadcastStream::new(state.app.frpc.subscribe()).filter_map(|result| match result {
        Ok(event) => Some(Ok(
            axum::response::sse::Event::default()
                .event("frpc")
                .json_data(event)
                .unwrap_or_else(|_| axum::response::sse::Event::default().data("{}")),
        )),
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}
