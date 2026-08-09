use crate::{coordinator::Coordinator,reconcile,state::AppState};
use ashan_frp_domain::{ApiResponse,DashboardSnapshot,ErrorResponse,ProviderHealth,RoutingUpdate,TunnelPlanInput};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response,Sse},routing::{get,post},Json,Router};
use futures_util::Stream;
use std::{convert::Infallible,sync::Arc,time::Duration};
use tokio_stream::{wrappers::BroadcastStream,StreamExt};

#[derive(Clone)] struct ApiState { app:Arc<AppState>, coordinator:Coordinator }
pub fn router(app:Arc<AppState>,coordinator:Coordinator)->Router{let s=ApiState{app,coordinator};Router::new()
    .route("/api/v1/health",get(health)).route("/api/v1/dashboard",get(dashboard)).route("/api/v1/tunnels",get(list_tunnels).post(create_tunnel))
    .route("/api/v1/tunnels/{id}",put(update_tunnel).delete(delete_tunnel)).route("/api/v1/routing",get(routing).put(update_routing))
    .route("/api/v1/nodes",get(nodes)).route("/api/v1/reconcile",post(reconcile_all)).route("/api/v1/failover",post(manual_failover))
    .route("/api/v1/frpc/status",get(frpc_status)).route("/api/v1/frpc/logs",get(frpc_logs)).route("/api/v1/frpc/start",post(frpc_start)).route("/api/v1/frpc/stop",post(frpc_stop)).route("/api/v1/frpc/restart",post(frpc_restart))
    .route("/api/v1/events",get(events)).with_state(s)}

type ApiResult<T>=Result<Json<ApiResponse<T>>,ApiError>;
struct ApiError(anyhow::Error); impl From<anyhow::Error> for ApiError{fn from(e:anyhow::Error)->Self{Self(e)}}
impl IntoResponse for ApiError{fn into_response(self)->Response{(StatusCode::INTERNAL_SERVER_ERROR,Json(ErrorResponse{code:"INTERNAL_ERROR".into(),message:self.0.to_string()})).into_response()}}

async fn health(State(s):State<ApiState>)->ApiResult<serde_json::Value>{Ok(Json(ApiResponse{data:serde_json::json!({"status":"ok","version":env!("CARGO_PKG_VERSION"),"frpc":s.app.frpc.status().await.running})}))}
async fn dashboard(State(s):State<ApiState>)->ApiResult<DashboardSnapshot>{
    let routing=s.app.db.routing_state().await?;let mut nodes=s.app.chml.list_nodes().await.unwrap_or_default();for n in &mut nodes{n.quarantined_until=s.app.db.quarantine_until(&n.name).await?;}
    let active=routing.active_node.as_ref().and_then(|name|nodes.iter().find(|n|&n.name==name).cloned());let standby=routing.standby_node.as_ref().and_then(|name|nodes.iter().find(|n|&n.name==name).cloned());
    let ch=provider_health(s.app.chml.configured(),s.app.chml.health().await);let cf=provider_health(s.app.cf.configured(),s.app.cf.health().await);
    Ok(Json(ApiResponse{data:DashboardSnapshot{routing,active_node:active,standby_node:standby,frpc:s.app.frpc.status().await,chmlfrp_health:ch,cloudflare_health:cf,tunnel_rows:reconcile::build_rows(&s.app).await?,recent_activity:s.app.db.recent_activity(50).await?,failover_job_id:s.app.failover_job.read().await.clone()}}))
}
fn provider_health(configured:bool,r:anyhow::Result<()>)->ProviderHealth{match r{Ok(_)=>ProviderHealth{configured,connected:true,message:"已连接".into()},Err(e)=>ProviderHealth{configured,connected:false,message:e.to_string()}}}
async fn list_tunnels(State(s):State<ApiState>)->ApiResult<Vec<ashan_frp_domain::TunnelPlan>>{Ok(Json(ApiResponse{data:s.app.db.list_tunnels().await?}))}
async fn create_tunnel(State(s):State<ApiState>,Json(i):Json<TunnelPlanInput>)->ApiResult<ashan_frp_domain::TunnelPlan>{validate(&i)?;Ok(Json(ApiResponse{data:s.app.db.create_tunnel(&i).await?}))}
async fn update_tunnel(State(s):State<ApiState>,Path(id):Path<i64>,Json(i):Json<TunnelPlanInput>)->ApiResult<ashan_frp_domain::TunnelPlan>{validate(&i)?;Ok(Json(ApiResponse{data:s.app.db.update_tunnel(id,&i).await?}))}
async fn delete_tunnel(State(s):State<ApiState>,Path(id):Path<i64>)->ApiResult<serde_json::Value>{s.app.db.delete_tunnel(id).await?;Ok(Json(ApiResponse{data:serde_json::json!({"deleted":id})}))}
fn validate(i:&TunnelPlanInput)->Result<(),ApiError>{if i.name.trim().is_empty()||i.local_ip.trim().is_empty()||i.domain.trim().is_empty(){return Err(ApiError(anyhow::anyhow!("name/local_ip/domain are required")))}if !(1..=65535).contains(&i.local_port){return Err(ApiError(anyhow::anyhow!("local_port must be 1..65535")))}Ok(())}
async fn routing(State(s):State<ApiState>)->ApiResult<ashan_frp_domain::RoutingState>{Ok(Json(ApiResponse{data:s.app.db.routing_state().await?}))}
async fn update_routing(State(s):State<ApiState>,Json(i):Json<RoutingUpdate>)->ApiResult<ashan_frp_domain::RoutingState>{
    if !(1..=3650).contains(&i.quarantine_days){return Err(ApiError(anyhow::anyhow!("quarantine_days must be 1..3650")))}
    if i.active_node.is_some() && i.active_node==i.standby_node{return Err(ApiError(anyhow::anyhow!("active_node and standby_node must differ")))}
    let current=s.app.db.routing_state().await?;
    if current.active_node.is_some() && current.active_node!=i.active_node{
        return Err(ApiError(anyhow::anyhow!("active_node cannot be changed directly; use GLOBAL_FAILOVER so all tunnels move together")));
    }
    Ok(Json(ApiResponse{data:s.app.db.update_routing(&i).await?}))
}
async fn nodes(State(s):State<ApiState>)->ApiResult<Vec<ashan_frp_domain::NodeSummary>>{let mut n=s.app.chml.list_nodes().await?;for x in &mut n{x.quarantined_until=s.app.db.quarantine_until(&x.name).await?;}Ok(Json(ApiResponse{data:n}))}
async fn reconcile_all(State(s):State<ApiState>)->ApiResult<serde_json::Value>{let job=s.coordinator.reconcile().await?;Ok(Json(ApiResponse{data:serde_json::json!({"job_id":job})}))}
async fn manual_failover(State(s):State<ApiState>)->ApiResult<serde_json::Value>{let job=s.coordinator.global_failover(None).await?;Ok(Json(ApiResponse{data:serde_json::json!({"job_id":job})}))}
async fn frpc_status(State(s):State<ApiState>)->ApiResult<ashan_frp_domain::FrpcRuntimeStatus>{Ok(Json(ApiResponse{data:s.app.frpc.status().await}))}
async fn frpc_logs(State(s):State<ApiState>)->ApiResult<Vec<ashan_frp_domain::FrpcEvent>>{Ok(Json(ApiResponse{data:s.app.frpc.recent_logs(500).await}))}
async fn frpc_start(State(s):State<ApiState>)->ApiResult<serde_json::Value>{s.app.frpc.start().await?;Ok(Json(ApiResponse{data:serde_json::json!({"ok":true})}))}
async fn frpc_stop(State(s):State<ApiState>)->ApiResult<serde_json::Value>{s.app.frpc.stop().await?;Ok(Json(ApiResponse{data:serde_json::json!({"ok":true})}))}
async fn frpc_restart(State(s):State<ApiState>)->ApiResult<serde_json::Value>{s.app.frpc.restart().await?;Ok(Json(ApiResponse{data:serde_json::json!({"ok":true})}))}
async fn events(State(s):State<ApiState>)->Sse<impl Stream<Item=Result<axum::response::sse::Event,Infallible>>>{let rx=s.app.frpc.subscribe();let stream=BroadcastStream::new(rx).filter_map(|r|match r{Ok(e)=>Some(Ok(axum::response::sse::Event::default().event("frpc").json_data(e).unwrap_or_else(|_|axum::response::sse::Event::default().data("{}")))),Err(_)=>None});Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15)).text("keepalive"))}
