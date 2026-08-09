use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayerState {
    Ok,
    Missing,
    Drift,
    Starting,
    Failed,
    Waiting,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayerStatus {
    pub state: LayerState,
    pub label: String,
    pub detail: Option<String>,
}

impl LayerStatus {
    pub fn ok(label: impl Into<String>) -> Self {
        Self { state: LayerState::Ok, label: label.into(), detail: None }
    }
    pub fn unknown(label: impl Into<String>) -> Self {
        Self { state: LayerState::Unknown, label: label.into(), detail: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TunnelPlan {
    pub id: i64,
    pub name: String,
    pub local_ip: String,
    pub local_port: i64,
    pub protocol: String,
    pub domain: String,
    pub dns_managed: bool,
    pub cloudflare_record_id: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TunnelPlanInput {
    pub name: String,
    pub local_ip: String,
    pub local_port: i64,
    pub protocol: String,
    pub domain: String,
    pub dns_managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TunnelRow {
    pub plan: TunnelPlan,
    pub chmlfrp: LayerStatus,
    pub frpc: LayerStatus,
    pub cloudflare: LayerStatus,
    pub overall: LayerStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutingState {
    pub active_node: Option<String>,
    pub standby_node: Option<String>,
    pub quarantine_days: i64,
    pub failover_enabled: bool,
    pub state: String,
    pub revision: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutingUpdate {
    pub active_node: Option<String>,
    pub standby_node: Option<String>,
    pub quarantine_days: i64,
    pub failover_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeSummary {
    pub id: i64,
    pub name: String,
    pub area: String,
    pub web_supported: bool,
    pub state: String,
    pub real_ip: Option<String>,
    pub host: Option<String>,
    pub load1: Option<f64>,
    pub bandwidth_usage_percent: Option<f64>,
    pub quarantined_until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FaultDomain {
    Local,
    Config,
    Auth,
    Network,
    Node,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrpcEventType {
    LoginSuccess,
    ProxyAdded,
    ProxyStarted,
    LocalServiceFailure,
    ConfigMismatch,
    LocalDuplicateProxy,
    ServerProxyConflict,
    ServerConnectionFailure,
    AuthFailure,
    NetworkAmbiguous,
    Info,
    UnknownError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrpcEvent {
    pub at: DateTime<Utc>,
    pub raw: String,
    pub proxy_name: Option<String>,
    pub event_type: FrpcEventType,
    pub fault_domain: FaultDomain,
    pub severity: String,
    pub triggers_failover: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrpcTunnelState {
    pub tunnel_name: String,
    pub state: LayerStatus,
    pub last_event: Option<FrpcEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrpcRuntimeStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub connected: bool,
    pub config_path: String,
    pub config_revision: Option<i64>,
    pub started_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderHealth {
    pub configured: bool,
    pub connected: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityEvent {
    pub id: i64,
    pub job_id: Option<String>,
    pub kind: String,
    pub level: String,
    pub message: String,
    pub tunnel_name: Option<String>,
    pub node_name: Option<String>,
    pub details: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardSnapshot {
    pub routing: RoutingState,
    pub active_node: Option<NodeSummary>,
    pub standby_node: Option<NodeSummary>,
    pub frpc: FrpcRuntimeStatus,
    pub chmlfrp_health: ProviderHealth,
    pub cloudflare_health: ProviderHealth,
    pub tunnel_rows: Vec<TunnelRow>,
    pub recent_activity: Vec<ActivityEvent>,
    pub failover_job_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiResponse<T> {
    pub data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
}
