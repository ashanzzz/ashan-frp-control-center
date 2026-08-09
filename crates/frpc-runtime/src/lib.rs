use anyhow::{anyhow, Context, Result};
use ashan_frp_domain::{
    FrpcEvent, FrpcEventType, FrpcRuntimeStatus, FrpcTunnelState, LayerState, LayerStatus,
};
use ashan_frp_frpc_log::{classify, config_has_duplicate_proxy_names};
use chrono::Utc;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{broadcast, Mutex, RwLock},
};

#[derive(Clone)]
pub struct FrpcManager {
    binary: PathBuf,
    config: PathBuf,
    child: Arc<Mutex<Option<Child>>>,
    runtime: Arc<RwLock<FrpcRuntimeStatus>>,
    tunnel_states: Arc<RwLock<HashMap<String, FrpcTunnelState>>>,
    logs: Arc<RwLock<VecDeque<FrpcEvent>>>,
    tx: broadcast::Sender<FrpcEvent>,
    max_logs: usize,
}

impl FrpcManager {
    pub fn new(
        binary: impl Into<PathBuf>,
        config: impl Into<PathBuf>,
        max_logs: usize,
    ) -> Self {
        let binary = binary.into();
        let config = config.into();
        let (tx, _) = broadcast::channel(1024);
        Self {
            binary,
            config: config.clone(),
            child: Arc::new(Mutex::new(None)),
            runtime: Arc::new(RwLock::new(FrpcRuntimeStatus {
                running: false,
                pid: None,
                connected: false,
                config_path: config.display().to_string(),
                config_revision: None,
                started_at: None,
                last_error: None,
            })),
            tunnel_states: Arc::new(RwLock::new(HashMap::new())),
            logs: Arc::new(RwLock::new(VecDeque::new())),
            tx,
            max_logs: max_logs.max(100),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<FrpcEvent> {
        self.tx.subscribe()
    }

    pub async fn status(&self) -> FrpcRuntimeStatus {
        let mut exit_reason = None;
        {
            let mut guard = self.child.lock().await;
            if let Some(child) = guard.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => exit_reason = Some(format!("FRPC exited: {status}")),
                    Ok(None) => {}
                    Err(err) => exit_reason = Some(format!("FRPC status check failed: {err}")),
                }
            }
            if exit_reason.is_some() {
                *guard = None;
            }
        }
        if let Some(reason) = exit_reason {
            let mut runtime = self.runtime.write().await;
            runtime.running = false;
            runtime.pid = None;
            runtime.connected = false;
            runtime.last_error = Some(reason);
        }
        self.runtime.read().await.clone()
    }

    pub async fn tunnel_states(&self) -> HashMap<String, FrpcTunnelState> {
        self.tunnel_states.read().await.clone()
    }

    pub async fn recent_logs(&self, limit: usize) -> Vec<FrpcEvent> {
        let logs = self.logs.read().await;
        logs.iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub async fn write_config(&self, text: &str, revision: i64) -> Result<()> {
        if let Some(parent) = self.config.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = self.config.with_extension("tmp");
        tokio::fs::write(&tmp, text).await?;
        tokio::fs::rename(&tmp, &self.config).await?;
        self.runtime.write().await.config_revision = Some(revision);
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        if !self.binary.exists() {
            return Err(anyhow!("FRPC binary not found: {}", self.binary.display()));
        }
        if !self.config.exists() {
            return Err(anyhow!("FRPC config not found: {}", self.config.display()));
        }

        let config_text = tokio::fs::read_to_string(&self.config)
            .await
            .unwrap_or_default();
        let local_dup = config_has_duplicate_proxy_names(&config_text);

        let mut guard = self.child.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        // A new complete ChmlFrp configuration is a new runtime generation.
        // Never carry old per-tunnel success states across a restart.
        self.tunnel_states.write().await.clear();

        let mut cmd = Command::new(&self.binary);
        cmd.arg("-c")
            .arg(&self.config)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .with_context(|| format!("start {}", self.binary.display()))?;
        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        {
            let mut runtime = self.runtime.write().await;
            runtime.running = true;
            runtime.pid = pid;
            runtime.connected = false;
            runtime.started_at = Some(Utc::now().to_rfc3339());
            runtime.last_error = None;
        }

        if let Some(stdout) = stdout {
            self.spawn_reader(stdout, local_dup);
        }
        if let Some(stderr) = stderr {
            self.spawn_reader(stderr, local_dup);
        }
        *guard = Some(child);
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let mut runtime = self.runtime.write().await;
        runtime.running = false;
        runtime.pid = None;
        runtime.connected = false;
        Ok(())
    }

    pub async fn restart(&self) -> Result<()> {
        self.stop().await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.start().await
    }

    /// Wait until the new complete ChmlFrp-generated configuration is actually observed
    /// as connected and every expected tunnel has emitted a successful startup event.
    ///
    /// ChmlFrp/frpc deployments may prefix proxy names (for example token.name), so
    /// matching accepts either an exact plan name or a `.plan-name` suffix.
    pub async fn wait_ready(&self, expected_tunnels: &[String], timeout: Duration) -> Result<()> {
        let started = Instant::now();
        loop {
            let runtime = self.status().await;
            if !runtime.running {
                return Err(anyhow!(
                    "FRPC stopped while validating target node: {}",
                    runtime.last_error.unwrap_or_else(|| "unknown exit".into())
                ));
            }

            if runtime.connected {
                let states = self.tunnel_states().await;
                let all_started = expected_tunnels.iter().all(|expected| {
                    states.iter().any(|(actual, state)| {
                        proxy_name_matches(actual, expected) && state.state.state == LayerState::Ok
                    })
                });
                if expected_tunnels.is_empty() || all_started {
                    return Ok(());
                }
            }

            if started.elapsed() >= timeout {
                let states = self.tunnel_states().await;
                let missing = expected_tunnels
                    .iter()
                    .filter(|expected| {
                        !states.iter().any(|(actual, state)| {
                            proxy_name_matches(actual, expected)
                                && state.state.state == LayerState::Ok
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                return Err(anyhow!(
                    "FRPC readiness timeout; connected={}, tunnels_not_ready={}",
                    runtime.connected,
                    missing.join(",")
                ));
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    fn spawn_reader<R>(&self, reader: R, local_dup: bool)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let this = self.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                this.accept_line(line, local_dup).await;
            }
        });
    }

    async fn accept_line(&self, line: String, local_dup: bool) {
        let event = classify(&line, local_dup);
        {
            let mut runtime = self.runtime.write().await;
            match event.event_type {
                FrpcEventType::LoginSuccess => {
                    runtime.connected = true;
                    runtime.last_error = None;
                }
                FrpcEventType::ServerConnectionFailure => {
                    runtime.connected = false;
                    runtime.last_error = Some(event.raw.clone());
                }
                FrpcEventType::AuthFailure => {
                    runtime.last_error = Some(event.raw.clone());
                }
                _ => {}
            }
        }

        // A confirmed server/node fault has global scope in this product.  Every
        // managed proxy uses the same ChmlFrp node, so do not leave stale green
        // tunnel rows while the global failover is being started.
        if event.triggers_failover && event.fault_domain == ashan_frp_domain::FaultDomain::Node {
            let mut states = self.tunnel_states.write().await;
            for state in states.values_mut() {
                state.state = LayerStatus {
                    state: LayerState::Failed,
                    label: "节点故障".into(),
                    detail: Some(event.raw.clone()),
                };
                state.last_event = Some(event.clone());
            }
        }

        if let Some(name) = event.proxy_name.clone() {
            let state = match event.event_type {
                FrpcEventType::ProxyStarted => Some(LayerStatus::ok("正常")),
                FrpcEventType::ProxyAdded => Some(LayerStatus {
                    state: LayerState::Starting,
                    label: "启动中".into(),
                    detail: None,
                }),
                FrpcEventType::LocalServiceFailure => Some(LayerStatus {
                    state: LayerState::Failed,
                    label: "本地服务异常".into(),
                    detail: Some(event.raw.clone()),
                }),
                FrpcEventType::ServerProxyConflict => Some(LayerStatus {
                    state: LayerState::Failed,
                    label: "服务端冲突".into(),
                    detail: Some(event.raw.clone()),
                }),
                FrpcEventType::ServerConnectionFailure => Some(LayerStatus {
                    state: LayerState::Failed,
                    label: "节点故障".into(),
                    detail: Some(event.raw.clone()),
                }),
                FrpcEventType::ConfigMismatch | FrpcEventType::LocalDuplicateProxy => Some(LayerStatus {
                    state: LayerState::Failed,
                    label: "配置异常".into(),
                    detail: Some(event.raw.clone()),
                }),
                _ => None,
            };
            if let Some(state) = state {
                self.tunnel_states.write().await.insert(
                    name.clone(),
                    FrpcTunnelState {
                        tunnel_name: name,
                        state,
                        last_event: Some(event.clone()),
                    },
                );
            }
        }

        {
            let mut logs = self.logs.write().await;
            logs.push_back(event.clone());
            while logs.len() > self.max_logs {
                logs.pop_front();
            }
        }
        let _ = self.tx.send(event);
    }
}

fn proxy_name_matches(actual: &str, expected: &str) -> bool {
    actual == expected || actual.ends_with(&format!(".{expected}"))
}

#[cfg(test)]
mod tests {
    use super::proxy_name_matches;

    #[test]
    fn accepts_exact_and_chmlfrp_prefixed_proxy_names() {
        assert!(proxy_name_matches("new-api", "new-api"));
        assert!(proxy_name_matches("token.new-api", "new-api"));
        assert!(!proxy_name_matches("new-api-copy", "new-api"));
    }
}
