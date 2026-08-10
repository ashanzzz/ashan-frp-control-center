mod api;
mod coordinator;
mod reconcile;
mod state;

use anyhow::{Context, Result};
use ashan_frp_chmlfrp::ChmlFrpClient;
use ashan_frp_cloudflare::CloudflareClient;
use ashan_frp_database::Database;
use ashan_frp_frpc_runtime::FrpcManager;
use coordinator::Coordinator;
use state::{AppConfig, AppState};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let config = AppConfig::from_env()?;
    tokio::fs::create_dir_all(&config.data_dir)
        .await
        .context("create data directory")?;

    let db = Database::connect(&config.database_url).await?;
    db.seed_routing_from_env(
        config.active_node.as_deref(),
        config.standby_node.as_deref(),
        config.quarantine_days,
        config.failover_enabled,
    )
    .await?;

    let state = Arc::new(AppState {
        db,
        chml: ChmlFrpClient::new(&config.chmlfrp_base_url, &config.chmlfrp_token)?,
        cf: CloudflareClient::new(
            &config.cloudflare_api_base,
            &config.cloudflare_api_token,
            &config.cloudflare_zone_id,
        )?,
        frpc: FrpcManager::new(
            &config.frpc_binary,
            &config.frpc_config,
            config.frpc_log_tail,
        ),
        failover_job: Arc::new(RwLock::new(None)),
    });
    let coordinator = Coordinator::new(state.clone());
    coordinator.spawn_frpc_fault_watcher();

    restore_frpc_if_possible(&state, &config).await;

    let index = format!("{}/index.html", config.web_dir.trim_end_matches('/'));
    let static_files = ServeDir::new(&config.web_dir).not_found_service(ServeFile::new(index));
    let app = api::router(state.clone(), coordinator)
        .fallback_service(static_files)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&config.http_addr)
        .await
        .with_context(|| format!("bind {}", config.http_addr))?;
    info!(addr = %config.http_addr, version = env!("CARGO_PKG_VERSION"), "control center started");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve HTTP")?;

    if let Err(error) = state.frpc.stop().await {
        warn!(error = %error, "failed to stop FRPC during shutdown");
    }
    info!("control center stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            error!(error = %error, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                error!(error = %error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

async fn restore_frpc_if_possible(state: &Arc<AppState>, config: &AppConfig) {
    let has_binary = tokio::fs::try_exists(&config.frpc_binary)
        .await
        .unwrap_or(false);
    let has_config = tokio::fs::try_exists(&config.frpc_config)
        .await
        .unwrap_or(false);

    if has_binary && has_config {
        match state.frpc.start().await {
            Ok(()) => info!(generation = state.frpc.generation(), "restored FRPC runtime"),
            Err(error) => error!(error = %error, "FRPC restore failed; WebUI remains available"),
        }
    } else {
        warn!(has_binary, has_config, "FRPC auto-start skipped");
    }
}
