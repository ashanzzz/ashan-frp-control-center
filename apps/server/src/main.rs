mod api;
mod coordinator;
mod reconcile;
mod state;

use anyhow::Result;
use ashan_frp_chmlfrp::ChmlFrpClient;
use ashan_frp_cloudflare::CloudflareClient;
use ashan_frp_database::Database;
use ashan_frp_frpc_runtime::FrpcManager;
use coordinator::Coordinator;
use state::{AppConfig, AppState};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::{services::{ServeDir,ServeFile}, trace::TraceLayer};
use tracing::{error,info,warn};

#[tokio::main]
async fn main()->Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).json().init();
    let cfg=AppConfig::from_env();
    tokio::fs::create_dir_all(&cfg.data_dir).await?;
    let db=Database::connect(&cfg.database_url).await?;
    db.seed_routing_from_env(cfg.active_node.as_deref(),cfg.standby_node.as_deref(),cfg.quarantine_days,cfg.failover_enabled).await?;
    let chml=ChmlFrpClient::new(&cfg.chmlfrp_base_url,&cfg.chmlfrp_token);
    let cf=CloudflareClient::new(&cfg.cloudflare_api_base,&cfg.cloudflare_api_token,&cfg.cloudflare_zone_id);
    let frpc=FrpcManager::new(&cfg.frpc_binary,&cfg.frpc_config,cfg.frpc_log_tail);
    let state=Arc::new(AppState{db,chml,cf,frpc,failover_job:Arc::new(RwLock::new(None))});
    let coordinator=Coordinator::new(state.clone());
    coordinator.spawn_frpc_fault_watcher();

    // High availability must survive a container restart.  If the last
    // ChmlFrp-generated config and the frpc binary are already present, start the
    // managed runtime automatically.  A runtime failure is reported but must not
    // prevent the control console from coming up for recovery.
    let has_binary=tokio::fs::try_exists(&cfg.frpc_binary).await.unwrap_or(false);
    let has_config=tokio::fs::try_exists(&cfg.frpc_config).await.unwrap_or(false);
    if has_binary && has_config {
        match state.frpc.start().await {
            Ok(())=>info!(generation=state.frpc.generation(),"restored FRPC runtime from persisted ChmlFrp config"),
            Err(err)=>error!(error=%err,"failed to restore FRPC runtime; control console remains available"),
        }
    } else {
        warn!(has_binary,has_config,"FRPC auto-start skipped until binary and ChmlFrp-generated config are available");
    }

    let api_router=api::router(state.clone(),coordinator.clone());
    let index=format!("{}/index.html",cfg.web_dir.trim_end_matches('/'));
    let static_service=ServeDir::new(&cfg.web_dir).not_found_service(ServeFile::new(index));
    let app=api_router.fallback_service(static_service).layer(TraceLayer::new_for_http());
    let listener=tokio::net::TcpListener::bind(&cfg.http_addr).await?;
    info!(addr=%cfg.http_addr,"Ashan FRP Control Center started");
    if let Err(e)=axum::serve(listener,app).await { error!(error=%e,"server stopped"); }
    Ok(())
}
