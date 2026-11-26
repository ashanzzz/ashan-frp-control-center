use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
        Self {
            state: LayerState::Ok,
            label: label.into(),
            detail: None,
        }
    }

    pub fn unknown(label: impl Into<String>) -> Self {
        Self {
            state: LayerState::Unknown,
            label: label.into(),
            detail: None,
        }
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutingPhase {
    Idle,
    Failover,
    DnsSwitching,
    DegradedDns,
    Failed,
}

impl RoutingPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Failover => "failover",
            Self::DnsSwitching => "dns_switching",
            Self::DegradedDns => "degraded_dns",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for RoutingPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RoutingPhase {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "idle" => Ok(Self::Idle),
            "failover" => Ok(Self::Failover),
            "dns_switching" => Ok(Self::DnsSwitching),
            "degraded_dns" => Ok(Self::DegradedDns),
            "failed" => Ok(Self::Failed),
            other => Err(format!("unknown routing phase: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutingState {
    pub active_node: Option<String>,
    pub standby_node: Option<String>,
    pub quarantine_days: i64,
    pub failover_enabled: bool,
    pub state: RoutingPhase,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeTag {
    pub code: String,
    pub label: String,
    pub tone: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodePolicyFlags {
    pub attacked: bool,
    pub forbids_drive: bool,
    pub high_bandwidth: bool,
    pub bandwidth_limited: bool,
    pub requires_icp: bool,
    pub requires_whitelist: bool,
    pub bans_overseas: bool,
    pub high_load: bool,
    pub unstable_hint: bool,
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
    #[serde(default)]
    pub frps_port: Option<u16>,
    #[serde(default)]
    pub nodegroup: Option<String>,
    #[serde(default)]
    pub china: Option<bool>,
    #[serde(default)]
    pub fangyu: Option<bool>,
    #[serde(default)]
    pub udp: Option<bool>,
    #[serde(default)]
    pub toowhite: Option<bool>,
    pub load1: Option<f64>,
    #[serde(default)]
    pub load5: Option<f64>,
    #[serde(default)]
    pub load15: Option<f64>,
    pub bandwidth_usage_percent: Option<f64>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<NodeTag>,
    #[serde(default)]
    pub policy: NodePolicyFlags,
    pub quarantined_until: Option<String>,
}

impl NodeSummary {
    /// Parse ChmlFrp node `notes` and telemetry (`load1`, `fangyu`, `china`, etc.)
    /// into machine-readable `policy` flags and display `tags`.
    pub fn classify(&mut self) {
        let notes = self.notes.as_str();
        let attacked = notes.contains("节点被攻击") || notes.contains("暂时无法使用");
        let forbids_drive = notes.contains("禁止大流量")
            || notes.contains("禁止用于文件传输")
            || (notes.contains("禁止") && notes.contains("网盘"));
        let bandwidth_limited = notes.contains("高峰期限速")
            || notes.contains("限速严重")
            || notes.contains("线路较差");
        let high_bandwidth = !forbids_drive
            && !bandwidth_limited
            && (notes.contains("超大带宽")
                || notes.contains("超高带宽")
                || notes.contains("10G")
                || notes.contains("AS4837")
                || notes.contains("大带宽")
                || notes.contains("跑满带宽")
                || notes.contains("强烈建议文件传输"));
        let requires_icp = notes.contains("备案");
        let requires_whitelist = self.toowhite == Some(true) || notes.contains("过白");
        let bans_overseas = notes.contains("ban海外") || notes.contains("封海外");
        let high_load = self.load1.is_some_and(|l| l >= 3.0)
            || self.bandwidth_usage_percent.is_some_and(|b| b >= 85.0)
            || notes.contains("负载较高")
            || notes.contains("负载较大");
        let unstable_hint = notes.contains("每日凌晨中断")
            || notes.contains("动态ip")
            || notes.contains("晚高峰较差");

        self.policy = NodePolicyFlags {
            attacked,
            forbids_drive,
            high_bandwidth,
            bandwidth_limited,
            requires_icp,
            requires_whitelist,
            bans_overseas,
            high_load,
            unstable_hint,
        };

        let mut tags = Vec::new();
        let push_tag = |tags: &mut Vec<NodeTag>, code: &str, label: &str, tone: &str| {
            tags.push(NodeTag {
                code: code.to_owned(),
                label: label.to_owned(),
                tone: tone.to_owned(),
            });
        };

        if attacked {
            push_tag(&mut tags, "attacked", "被攻击/不可用", "bad");
        }
        if self.web_supported {
            push_tag(&mut tags, "web", "可建站", "ok");
        } else {
            push_tag(&mut tags, "no_web", "不支持建站", "");
        }
        if high_bandwidth {
            let label = if notes.contains("10G") {
                "10G大带宽"
            } else if notes.contains("AS4837") {
                "AS4837超大带宽"
            } else {
                "大带宽"
            };
            push_tag(&mut tags, "high_bw", label, "ok");
        }
        if forbids_drive {
            push_tag(&mut tags, "no_drive", "禁网盘/大流量", "bad");
        }
        if bandwidth_limited {
            push_tag(&mut tags, "bw_limited", "限速/线路较差", "warn");
        }
        if high_load {
            push_tag(&mut tags, "high_load", "高负载", "bad");
        } else if !attacked && !unstable_hint && self.load1.is_some_and(|l| l < 1.5) {
            push_tag(&mut tags, "stable", "稳定低负载", "ok");
        }
        if unstable_hint {
            push_tag(&mut tags, "unstable", "波动/定时中断", "warn");
        }
        if requires_icp {
            push_tag(&mut tags, "icp", "需备案", "warn");
        } else if self.china == Some(false) {
            push_tag(&mut tags, "no_icp", "免备案", "");
        }
        if requires_whitelist {
            push_tag(&mut tags, "whitelist", "需过白", "warn");
        }
        if bans_overseas {
            push_tag(&mut tags, "ban_overseas", "封海外", "warn");
        }
        match self.fangyu {
            Some(true) => push_tag(&mut tags, "defense", "有防御", ""),
            Some(false) => push_tag(&mut tags, "no_defense", "无防御", "warn"),
            None => {}
        }
        if self
            .nodegroup
            .as_deref()
            .is_some_and(|g| g.eq_ignore_ascii_case("vip"))
        {
            push_tag(&mut tags, "vip", "VIP", "ok");
        }

        self.tags = tags;
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FaultDomain {
    Local,
    Config,
    Auth,
    Network,
    Node,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
    #[serde(default)]
    pub runtime_generation: u64,
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
pub struct ProviderSettingsView {
    pub chmlfrp_base_url: String,
    pub chmlfrp_token_configured: bool,
    pub chmlfrp_refresh_token_configured: bool,
    pub chmlfrp_token_expires_at: i64,
    pub chmlfrp_username: String,
    pub chmlfrp_password_configured: bool,
    pub chmlfrp_totp_configured: bool,
    pub cloudflare_api_base: String,
    pub cloudflare_api_token_configured: bool,
    pub cloudflare_zone_id: String,
    pub updated_at: String,
    pub probe_port: u16,
    pub probe_timeout_ms: u64,
    pub probe_latency_tolerance_ms: u64,
    pub probe_require_high_bandwidth: bool,
    pub probe_exclude_icp_required: bool,
    pub probe_exclude_whitelist_and_ban_overseas: bool,
    pub probe_max_load1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderSettingsUpdate {
    pub chmlfrp_base_url: String,
    #[serde(default)]
    pub chmlfrp_token: Option<String>,
    #[serde(default)]
    pub clear_chmlfrp_token: bool,
    #[serde(default)]
    pub chmlfrp_refresh_token: Option<String>,
    #[serde(default)]
    pub chmlfrp_username: Option<String>,
    #[serde(default)]
    pub chmlfrp_password: Option<String>,
    #[serde(default)]
    pub chmlfrp_totp_secret: Option<String>,
    #[serde(default)]
    pub clear_chmlfrp_credentials: bool,
    pub cloudflare_api_base: String,
    #[serde(default)]
    pub cloudflare_api_token: Option<String>,
    #[serde(default)]
    pub clear_cloudflare_api_token: bool,
    pub cloudflare_zone_id: String,
    #[serde(default = "default_probe_port")]
    pub probe_port: u16,
    #[serde(default = "default_probe_timeout_ms")]
    pub probe_timeout_ms: u64,
    #[serde(default = "default_probe_latency_tolerance_ms")]
    pub probe_latency_tolerance_ms: u64,
    #[serde(default = "default_true")]
    pub probe_require_high_bandwidth: bool,
    #[serde(default)]
    pub probe_exclude_icp_required: bool,
    #[serde(default = "default_true")]
    pub probe_exclude_whitelist_and_ban_overseas: bool,
    #[serde(default = "default_probe_max_load1")]
    pub probe_max_load1: f64,
}

fn default_probe_port() -> u16 {
    7000
}
fn default_probe_timeout_ms() -> u64 {
    2000
}
fn default_probe_latency_tolerance_ms() -> u64 {
    50
}
fn default_true() -> bool {
    true
}
fn default_probe_max_load1() -> f64 {
    5.0
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

#[cfg(test)]
mod tests {
    use super::RoutingPhase;
    use std::str::FromStr;

    #[test]
    fn routing_phase_round_trip() {
        for phase in [
            RoutingPhase::Idle,
            RoutingPhase::Failover,
            RoutingPhase::DnsSwitching,
            RoutingPhase::DegradedDns,
            RoutingPhase::Failed,
        ] {
            assert_eq!(RoutingPhase::from_str(phase.as_str()).unwrap(), phase);
        }
    }

    #[test]
    fn classify_extracts_attack_drive_and_bandwidth_tags() {
        let mut sy = super::NodeSummary {
            id: 114,
            name: "沈阳电信".into(),
            area: "中国辽宁省沈阳市".into(),
            web_supported: true,
            state: "online".into(),
            real_ip: None,
            host: None,
            frps_port: Some(7000),
            nodegroup: Some("vip".into()),
            china: Some(true),
            fangyu: Some(false),
            udp: Some(true),
            toowhite: Some(false),
            load1: Some(0.04),
            load5: None,
            load15: None,
            bandwidth_usage_percent: Some(1.0),
            notes:
                "禁止任何高概率会被攻击业务，此节点禁止大流量（例如MC联机、网盘、长期大文件传输）"
                    .into(),
            tags: Vec::new(),
            policy: Default::default(),
            quarantined_until: None,
        };
        sy.classify();
        assert!(!sy.policy.attacked);
        assert!(sy.policy.forbids_drive);
        assert!(!sy.policy.high_bandwidth);
        assert!(sy.tags.iter().any(|t| t.code == "no_drive"));

        let mut bj = sy.clone();
        bj.notes = "节点被攻击，暂时无法使用。建站需备案接入腾讯云".into();
        bj.classify();
        assert!(bj.policy.attacked);
        assert!(bj.policy.requires_icp);

        let mut la3 = sy.clone();
        la3.notes = "超大带宽节点，中国大陆优化，AS4837线路。强烈建议文件传输使用".into();
        la3.china = Some(false);
        la3.fangyu = Some(true);
        la3.classify();
        assert!(la3.policy.high_bandwidth);
        assert!(!la3.policy.forbids_drive);
        assert!(la3.tags.iter().any(|t| t.code == "high_bw"));
        assert!(la3.tags.iter().any(|t| t.code == "no_icp"));
    }
}
