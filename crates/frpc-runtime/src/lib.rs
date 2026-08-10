use anyhow::{anyhow, Context, Result};
use ashan_frp_domain::{
    FaultDomain, FrpcEvent, FrpcEventType, FrpcRuntimeStatus, FrpcTunnelState, LayerState,
    LayerStatus,
};
use ashan_frp_frpc_log::{classify, config_has_duplicate_proxy_names};
use chrono::Utc;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{Mutex, RwLock, broadcast},
};

#[derive(Debug, Error)]
pub enum ReadinessError {
    #[error("node failure observed from FRPC: {0}")]
    Node(String),
    #[error("FRPC startup/configuration failure: {0}")]
    NonNode(String),
    #[error("FRPC readiness timeout; connected={connected}, tunnels_not_started={missing}")]
    Timeout { connected: bool, missing: String },
}

#[derive(Clone)]
pub struct FrpcManager {
    binary: PathBuf,
    config: PathBuf,
    child: Arc<Mutex<Option<Child>>>,
    runtime: Arc<RwLock<FrpcRuntimeStatus>>,
    tunnel_states: Arc<RwLock<HashMap<String, FrpcTunnelState>>>,
    started_proxies: Arc<RwLock<HashSet<String>>>,
    last_node_fault: Arc<RwLock<Option<FrpcEvent>>>,
    last_startup_fault: Arc<RwLock<Option<FrpcEvent>>>,
    logs: Arc<RwLock<VecDeque<FrpcEvent>>>,
    tx: broadcast::Sender<FrpcEvent>,
    max_logs: usize,
    generation: Arc<AtomicU64>,
}

impl FrpcManager {
    pub fn new(binary: impl Into<PathBuf>, config: impl Into<PathBuf>, max_logs: usize) -> Self {
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
            started_proxies: Arc::new(RwLock::new(HashSet::new())),
            last_node_fault: Arc::new(RwLock::new(None)),
            last_startup_fault: Arc::new(RwLock::new(None)),
            logs: Arc::new(RwLock::new(VecDeque::new())),
            tx,
            max_logs: max_logs.max(100),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<FrpcEvent> {
        self.tx.subscribe()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
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
        let mut recent = logs.iter().rev().take(limit).cloned().collect::<Vec<_>>();
        recent.reverse();
        recent
    }

    pub async fn read_config(&self) -> Result<Option<String>> {
        match tokio::fs::read_to_string(&self.config).await {
            Ok(text) => Ok(Some(text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn config_matches(&self, expected: &str) -> Result<bool> {
        Ok(self.read_config().await?.as_deref() == Some(expected))
    }

    pub async fn write_config(&self, text: &str, revision: i64) -> Result<()> {
        self.write_config_with_revision(text, Some(revision)).await
    }

    pub async fn restore_config(&self, text: &str, revision: Option<i64>) -> Result<()> {
        self.write_config_with_revision(text, revision).await
    }

    async fn write_config_with_revision(&self, text: &str, revision: Option<i64>) -> Result<()> {
        if let Some(parent) = self.config.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = self.config.with_extension("tmp");
        tokio::fs::write(&tmp, text).await?;
        tokio::fs::rename(&tmp, &self.config).await?;
        self.runtime.write().await.config_revision = revision;
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
            .with_context(|| format!("read FRPC config {}", self.config.display()))?;
        let local_dup = config_has_duplicate_proxy_names(&config_text);

        let mut guard = self.child.lock().await;
        if let Some(child) = guard.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    *guard = None;
                    let mut runtime = self.runtime.write().await;
                    runtime.running = false;
                    runtime.pid = None;
                    runtime.connected = false;
                    runtime.last_error =
                        Some(format!("FRPC exited before start request: {status}"));
                }
                Ok(None) => return Ok(()),
                Err(err) => return Err(anyhow!("FRPC process status check failed: {err}")),
            }
        }

        // Every start creates a new generation. Reader tasks from the previous
        // process may still drain buffered stdout/stderr after stop(); accept_line
        // rejects those stale lines by generation so they can never trigger a
        // failover against the newly active node.
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.tunnel_states.write().await.clear();
        self.started_proxies.write().await.clear();
        *self.last_node_fault.write().await = None;
        *self.last_startup_fault.write().await = None;

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
            self.spawn_reader(stdout, local_dup, generation);
        }
        if let Some(stderr) = stderr {
            self.spawn_reader(stderr, local_dup, generation);
        }
        *guard = Some(child);
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        // Invalidate reader tasks immediately. Buffered stdout/stderr from the
        // process being stopped must never be classified as current-runtime input.
        self.generation.fetch_add(1, Ordering::AcqRel);

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

    /// Wait until the new complete ChmlFrp-generated configuration is actually
    /// connected and every expected tunnel has emitted a successful startup event.
    ///
    /// Readiness is based on the generation-scoped `ProxyStarted` set, not the
    /// current UI status. A later local-service error may make one row red, but it
    /// must not falsely mark a healthy FRP node as a failed standby candidate.
    pub async fn wait_ready(
        &self,
        expected_tunnels: &[String],
        timeout: Duration,
    ) -> std::result::Result<(), ReadinessError> {
        let generation = self.generation();
        let started = Instant::now();
        loop {
            if self.generation() != generation {
                return Err(ReadinessError::NonNode(
                    "FRPC generation changed during readiness validation".into(),
                ));
            }

            let runtime = self.status().await;
            if let Some(event) = self.last_node_fault.read().await.clone()
                && event.runtime_generation == generation
            {
                return Err(ReadinessError::Node(event.raw));
            }
            if let Some(event) = self.last_startup_fault.read().await.clone()
                && event.runtime_generation == generation
                && matches!(event.fault_domain, FaultDomain::Auth | FaultDomain::Config)
            {
                return Err(ReadinessError::NonNode(event.raw));
            }
            if !runtime.running {
                return Err(ReadinessError::NonNode(runtime.last_error.unwrap_or_else(
                    || "FRPC exited during readiness validation".into(),
                )));
            }

            if runtime.connected {
                let started_names = self.started_proxies.read().await;
                let all_started = expected_tunnels.iter().all(|expected| {
                    started_names
                        .iter()
                        .any(|actual| proxy_name_matches(actual, expected))
                });
                if expected_tunnels.is_empty() || all_started {
                    return Ok(());
                }
            }

            if started.elapsed() >= timeout {
                let started_names = self.started_proxies.read().await;
                let missing = expected_tunnels
                    .iter()
                    .filter(|expected| {
                        !started_names
                            .iter()
                            .any(|actual| proxy_name_matches(actual, expected))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                return Err(ReadinessError::Timeout {
                    connected: runtime.connected,
                    missing: missing.join(","),
                });
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    fn spawn_reader<R>(&self, reader: R, local_dup: bool, generation: u64)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let this = self.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                this.accept_line(line, local_dup, generation).await;
            }
        });
    }

    async fn accept_line(&self, line: String, local_dup: bool, generation: u64) {
        if generation != self.generation() {
            return;
        }

        let mut event = classify(&line, local_dup);
        event.runtime_generation = generation;

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
                FrpcEventType::AuthFailure | FrpcEventType::ConfigMismatch => {
                    runtime.last_error = Some(event.raw.clone());
                }
                _ => {}
            }
        }

        if event.event_type == FrpcEventType::ProxyStarted
            && let Some(name) = event.proxy_name.as_ref()
        {
            self.started_proxies.write().await.insert(name.clone());
        }

        if event.triggers_failover && event.fault_domain == FaultDomain::Node {
            *self.last_node_fault.write().await = Some(event.clone());
            let mut states = self.tunnel_states.write().await;
            for state in states.values_mut() {
                state.state = LayerStatus {
                    state: LayerState::Failed,
                    label: "节点故障".into(),
                    detail: Some(event.raw.clone()),
                };
                state.last_event = Some(event.clone());
            }
        } else if matches!(event.fault_domain, FaultDomain::Auth | FaultDomain::Config) {
            *self.last_startup_fault.write().await = Some(event.clone());
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
                FrpcEventType::ConfigMismatch | FrpcEventType::LocalDuplicateProxy => {
                    Some(LayerStatus {
                        state: LayerState::Failed,
                        label: "配置异常".into(),
                        detail: Some(event.raw.clone()),
                    })
                }
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

    #[tokio::test]
    async fn stop_invalidates_the_current_runtime_generation() {
        let manager = super::FrpcManager::new("/missing/frpc", "/missing/frpc.ini", 100);
        let before = manager.generation();
        manager
            .stop()
            .await
            .expect("stop without child should succeed");
        assert!(manager.generation() > before);
    }
}
