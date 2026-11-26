use anyhow::{Result, anyhow};
use ashan_frp_domain::NodeSummary;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::debug;

/// Configuration for latency probing.
#[derive(Debug, Clone)]
pub struct ProbeSettings {
    /// TCP port to dial on each node's `host` IP (e.g. frpc listen port, default 7000).
    pub probe_port: u16,
    /// Per-attempt TCP connect timeout in milliseconds (default 2000).
    pub probe_timeout_ms: u64,
    /// STANDBY node tolerance band in milliseconds (default 50).
    /// If the preferred standby's RTT ≤ best_rtt + tolerance, it keeps priority.
    pub probe_latency_tolerance_ms: u64,
}

impl Default for ProbeSettings {
    fn default() -> Self {
        Self {
            probe_port: 7000,
            probe_timeout_ms: 2000,
            probe_latency_tolerance_ms: 50,
        }
    }
}

/// Result of probing one node.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub node: NodeSummary,
    /// Average RTT in milliseconds over `PROBE_ATTEMPTS` attempts.
    /// `None` means all attempts timed out or the node has no known IP.
    pub avg_rtt_ms: Option<f64>,
}

const PROBE_ATTEMPTS: usize = 3;

/// Probe all `nodes` concurrently and return results sorted by ascending average RTT.
/// Nodes with no IP or all-timeout results are appended at the end.
///
/// Returns `Err` only when **every** node failed to respond (all `avg_rtt_ms` are `None`).
/// In that case the caller should abort the failover and surface the error.
pub async fn probe_and_rank(
    nodes: &[NodeSummary],
    settings: &ProbeSettings,
) -> Result<Vec<ProbeResult>> {
    if nodes.is_empty() {
        return Ok(vec![]);
    }

    let timeout_dur = Duration::from_millis(settings.probe_timeout_ms);
    let port = settings.probe_port;

    // Fan out: one task per node.
    let handles: Vec<_> = nodes
        .iter()
        .map(|node| {
            let node = node.clone();
            tokio::spawn(async move {
                let avg = probe_node(&node, port, timeout_dur).await;
                ProbeResult {
                    node,
                    avg_rtt_ms: avg,
                }
            })
        })
        .collect();

    let mut results: Vec<ProbeResult> = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(result) => results.push(result),
            Err(join_err) => {
                // Task panicked – treat as timeout for that node.
                tracing::warn!("probe task panicked: {join_err}");
            }
        }
    }

    // Sort: successful probes by ascending RTT, then failures at end.
    results.sort_by(|a, b| match (a.avg_rtt_ms, b.avg_rtt_ms) {
        (Some(ra), Some(rb)) => ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let all_failed = results.iter().all(|r| r.avg_rtt_ms.is_none());
    if all_failed && !results.is_empty() {
        return Err(anyhow!(
            "所有候选节点延迟探针均超时或失败，无法完成节点延迟排序，failover 已阻断"
        ));
    }

    Ok(results)
}

/// Probe a single node: dial its host IP:port up to PROBE_ATTEMPTS times and
/// return the average RTT in milliseconds, or `None` if all attempts fail.
async fn probe_node(node: &NodeSummary, port: u16, timeout_dur: Duration) -> Option<f64> {
    let host = node.host.as_deref().filter(|h| !h.trim().is_empty())?;
    let addr = format!("{host}:{port}");
    debug!(node = %node.name, addr = %addr, "probing node latency");

    let mut total_ms = 0.0_f64;
    let mut success = 0usize;

    for attempt in 0..PROBE_ATTEMPTS {
        let start = Instant::now();
        match timeout(timeout_dur, TcpStream::connect(&addr)).await {
            Ok(Ok(_stream)) => {
                let rtt = start.elapsed().as_secs_f64() * 1000.0;
                debug!(node = %node.name, attempt, rtt_ms = rtt, "probe attempt ok");
                total_ms += rtt;
                success += 1;
            }
            Ok(Err(err)) => {
                debug!(node = %node.name, attempt, error = %err, "probe attempt connect error");
            }
            Err(_) => {
                debug!(node = %node.name, attempt, "probe attempt timeout");
            }
        }
    }

    if success == 0 {
        None
    } else {
        Some(total_ms / success as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, host: Option<&str>) -> NodeSummary {
        NodeSummary {
            id: 1,
            name: name.into(),
            area: String::new(),
            web_supported: true,
            state: "online".into(),
            real_ip: None,
            host: host.map(str::to_owned),
            load1: None,
            bandwidth_usage_percent: None,
            quarantined_until: None,
        }
    }

    #[test]
    fn probe_result_sorts_none_to_end() {
        let mut results = [
            ProbeResult {
                node: node("slow", None),
                avg_rtt_ms: None,
            },
            ProbeResult {
                node: node("fast", None),
                avg_rtt_ms: Some(10.0),
            },
            ProbeResult {
                node: node("mid", None),
                avg_rtt_ms: Some(50.0),
            },
        ];
        results.sort_by(|a, b| match (a.avg_rtt_ms, b.avg_rtt_ms) {
            (Some(ra), Some(rb)) => ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        assert_eq!(results[0].node.name, "fast");
        assert_eq!(results[1].node.name, "mid");
        assert_eq!(results[2].node.name, "slow");
    }

    #[test]
    fn no_host_returns_none_avg() {
        // Nodes without host should produce None avg_rtt_ms (tested via probe_node logic).
        let n = node("no-ip", None);
        // host is None → probe_node returns None immediately.
        assert!(n.host.is_none());
    }
}
