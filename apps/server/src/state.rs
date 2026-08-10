use anyhow::{Result, bail};
use ashan_frp_chmlfrp::ChmlFrpClient;
use ashan_frp_cloudflare::CloudflareClient;
use ashan_frp_database::Database;
use ashan_frp_frpc_runtime::FrpcManager;
use std::{env, sync::Arc};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub http_addr: String,
    pub data_dir: String,
    pub database_url: String,
    pub web_dir: String,
    pub chmlfrp_base_url: String,
    pub chmlfrp_token: String,
    pub cloudflare_api_base: String,
    pub cloudflare_api_token: String,
    pub cloudflare_zone_id: String,
    pub frpc_binary: String,
    pub frpc_config: String,
    pub frpc_log_tail: usize,
    pub active_node: Option<String>,
    pub standby_node: Option<String>,
    pub quarantine_days: i64,
    pub failover_enabled: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let data_dir = value("DATA_DIR", "/data");
        let quarantine_days = parse_env("QUARANTINE_DAYS", 30_i64)?;
        if !(1..=3650).contains(&quarantine_days) {
            bail!("QUARANTINE_DAYS must be 1..=3650");
        }
        let frpc_log_tail = parse_env("FRPC_LOG_TAIL", 500_usize)?;
        if !(100..=10_000).contains(&frpc_log_tail) {
            bail!("FRPC_LOG_TAIL must be 100..=10000");
        }

        Ok(Self {
            http_addr: value("HTTP_ADDR", "0.0.0.0:8080"),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| format!("sqlite://{data_dir}/control-center.db")),
            web_dir: value("WEB_DIR", "/app/web"),
            data_dir,
            chmlfrp_base_url: value("CHMLFRP_BASE_URL", "https://cf-v2.uapis.cn"),
            chmlfrp_token: env::var("CHMLFRP_TOKEN").unwrap_or_default(),
            cloudflare_api_base: value(
                "CLOUDFLARE_API_BASE",
                "https://api.cloudflare.com/client/v4",
            ),
            cloudflare_api_token: env::var("CLOUDFLARE_API_TOKEN").unwrap_or_default(),
            cloudflare_zone_id: env::var("CLOUDFLARE_ZONE_ID").unwrap_or_default(),
            frpc_binary: value("FRPC_BINARY", "/data/frpc/frpc"),
            frpc_config: value("FRPC_CONFIG", "/data/frpc/frpc.ini"),
            frpc_log_tail,
            active_node: optional("ACTIVE_NODE"),
            standby_node: optional("STANDBY_NODE"),
            quarantine_days,
            failover_enabled: parse_env("FAILOVER_ENABLED", true)?,
        })
    }
}

fn value(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn optional(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn parse_env<T>(key: &str, default: T) -> Result<T>
where
    T: std::str::FromStr + Copy,
    T::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(raw) => raw
            .parse()
            .map_err(|err| anyhow::anyhow!("parse {key}={raw}: {err}")),
        Err(_) => Ok(default),
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub chml: ChmlFrpClient,
    pub cf: CloudflareClient,
    pub frpc: FrpcManager,
    pub failover_job: Arc<RwLock<Option<String>>>,
}
