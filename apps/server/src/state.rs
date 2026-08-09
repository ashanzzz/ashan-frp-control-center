use ashan_frp_chmlfrp::ChmlFrpClient;
use ashan_frp_cloudflare::CloudflareClient;
use ashan_frp_database::Database;
use ashan_frp_frpc_runtime::FrpcManager;
use std::{env,sync::Arc};
use tokio::sync::RwLock;

#[derive(Debug,Clone)]
pub struct AppConfig {
    pub http_addr:String,pub data_dir:String,pub database_url:String,pub web_dir:String,
    pub chmlfrp_base_url:String,pub chmlfrp_token:String,pub cloudflare_api_base:String,pub cloudflare_api_token:String,pub cloudflare_zone_id:String,
    pub frpc_binary:String,pub frpc_config:String,pub frpc_log_tail:usize,pub active_node:Option<String>,pub standby_node:Option<String>,pub quarantine_days:i64,pub failover_enabled:bool,
}
impl AppConfig { pub fn from_env()->Self{ let data=val("DATA_DIR","/data"); Self{
    http_addr:val("HTTP_ADDR","0.0.0.0:8080"), database_url:env::var("DATABASE_URL").unwrap_or_else(|_|format!("sqlite://{data}/control-center.db")), web_dir:val("WEB_DIR","/app/web"), data_dir:data,
    chmlfrp_base_url:val("CHMLFRP_BASE_URL","https://cf-v2.uapis.cn"),chmlfrp_token:env::var("CHMLFRP_TOKEN").unwrap_or_default(),
    cloudflare_api_base:val("CLOUDFLARE_API_BASE","https://api.cloudflare.com/client/v4"),cloudflare_api_token:env::var("CLOUDFLARE_API_TOKEN").unwrap_or_default(),cloudflare_zone_id:env::var("CLOUDFLARE_ZONE_ID").unwrap_or_default(),
    frpc_binary:val("FRPC_BINARY","/data/frpc/frpc"),frpc_config:val("FRPC_CONFIG","/data/frpc/frpc.ini"),frpc_log_tail:val("FRPC_LOG_TAIL","500").parse().unwrap_or(500),
    active_node:opt("ACTIVE_NODE"),standby_node:opt("STANDBY_NODE"),quarantine_days:val("QUARANTINE_DAYS","30").parse().unwrap_or(30),failover_enabled:val("FAILOVER_ENABLED","true").parse().unwrap_or(true),
}}}
fn val(k:&str,d:&str)->String{env::var(k).unwrap_or_else(|_|d.into())} fn opt(k:&str)->Option<String>{env::var(k).ok().filter(|v|!v.trim().is_empty())}

#[derive(Clone)]
pub struct AppState { pub db:Database,pub chml:ChmlFrpClient,pub cf:CloudflareClient,pub frpc:FrpcManager,pub failover_job:Arc<RwLock<Option<String>>> }
