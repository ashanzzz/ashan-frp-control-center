use ashan_frp_domain::NodeSummary;

/// A latency probe result for one node.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub node_name: String,
    /// Average RTT in milliseconds; None means all probe attempts failed.
    pub avg_rtt_ms: Option<f64>,
}

pub fn ordered_candidates(
    active: &str,
    preferred_standby: Option<&str>,
    nodes: &[NodeSummary],
    require_web: bool,
) -> Vec<NodeSummary> {
    let mut out = Vec::new();
    if let Some(name) = preferred_standby
        && let Some(node) = nodes
            .iter()
            .find(|node| node.name == name && eligible(node, active, require_web))
    {
        out.push(node.clone());
    }
    for node in nodes {
        if eligible(node, active, require_web) && !out.iter().any(|n| n.name == node.name) {
            out.push(node.clone());
        }
    }
    out
}

/// Sort `candidates` by ascending probe RTT with STANDBY priority protection.
///
/// If the preferred standby node's RTT is within `tolerance_ms` of the best RTT,
/// it is kept at position 0. Otherwise, nodes are ordered by ascending RTT.
/// Nodes with no probe result (all timeouts) are appended at the end.
pub fn sort_by_latency(
    candidates: Vec<NodeSummary>,
    probe_results: &[ProbeResult],
    preferred_standby: Option<&str>,
    tolerance_ms: u64,
) -> Vec<NodeSummary> {
    if candidates.is_empty() {
        return candidates;
    }

    // Find the minimum RTT among all candidates that have a measurement.
    let min_rtt = candidates
        .iter()
        .filter_map(|n| {
            probe_results
                .iter()
                .find(|p| p.node_name == n.name)
                .and_then(|p| p.avg_rtt_ms)
        })
        .fold(f64::INFINITY, f64::min);

    // Check if STANDBY is eligible for position-0 protection.
    let standby_protected = preferred_standby.is_some_and(|standby| {
        candidates.iter().any(|n| n.name == standby)
            && probe_results
                .iter()
                .find(|p| p.node_name == standby)
                .and_then(|p| p.avg_rtt_ms)
                .is_some_and(|rtt| rtt <= min_rtt + tolerance_ms as f64)
    });

    let rtt_of = |name: &str| -> f64 {
        probe_results
            .iter()
            .find(|p| p.node_name == name)
            .and_then(|p| p.avg_rtt_ms)
            .unwrap_or(f64::INFINITY)
    };

    let mut result = candidates;
    result.sort_by(|a, b| {
        // If STANDBY is protected, pin it to front.
        if standby_protected {
            let a_standby = preferred_standby == Some(a.name.as_str());
            let b_standby = preferred_standby == Some(b.name.as_str());
            if a_standby && !b_standby {
                return std::cmp::Ordering::Less;
            }
            if b_standby && !a_standby {
                return std::cmp::Ordering::Greater;
            }
        }
        rtt_of(&a.name)
            .partial_cmp(&rtt_of(&b.name))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    result
}

pub fn choose_target(
    active: &str,
    preferred_standby: Option<&str>,
    nodes: &[NodeSummary],
    require_web: bool,
) -> Option<NodeSummary> {
    ordered_candidates(active, preferred_standby, nodes, require_web)
        .into_iter()
        .next()
}

fn eligible(node: &NodeSummary, active: &str, require_web: bool) -> bool {
    node.name != active
        && (!require_web || node.web_supported)
        && node.state.eq_ignore_ascii_case("online")
        && node.quarantined_until.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str) -> NodeSummary {
        NodeSummary {
            id: 1,
            name: name.into(),
            area: String::new(),
            web_supported: true,
            state: "online".into(),
            real_ip: None,
            host: None,
            load1: None,
            bandwidth_usage_percent: None,
            quarantined_until: None,
        }
    }

    fn probe(name: &str, rtt: Option<f64>) -> ProbeResult {
        ProbeResult {
            node_name: name.into(),
            avg_rtt_ms: rtt,
        }
    }

    #[test]
    fn never_returns_active() {
        assert!(choose_target("A", None, &[node("A")], true).is_none());
    }

    #[test]
    fn standby_is_first() {
        let nodes = vec![node("C"), node("B")];
        let result = ordered_candidates("A", Some("B"), &nodes, true);
        assert_eq!(result[0].name, "B");
    }

    #[test]
    fn tcp_only_routing_does_not_require_web_capability() {
        let mut tcp_node = node("B");
        tcp_node.web_supported = false;
        assert_eq!(
            choose_target("A", Some("B"), &[tcp_node], false)
                .expect("TCP-only candidate should be eligible")
                .name,
            "B"
        );
    }

    #[test]
    fn sort_by_latency_picks_fastest() {
        let candidates = vec![node("slow"), node("fast"), node("mid")];
        let probes = vec![
            probe("slow", Some(100.0)),
            probe("fast", Some(10.0)),
            probe("mid", Some(50.0)),
        ];
        let result = sort_by_latency(candidates, &probes, None, 50);
        assert_eq!(result[0].name, "fast");
        assert_eq!(result[1].name, "mid");
        assert_eq!(result[2].name, "slow");
    }

    #[test]
    fn standby_protected_within_tolerance() {
        // standby=B at 25ms, best=A at 10ms, tolerance=50ms → 25 ≤ 10+50 → B keeps #1
        let candidates = vec![node("A"), node("B"), node("C")];
        let probes = vec![
            probe("A", Some(10.0)),
            probe("B", Some(25.0)),
            probe("C", Some(15.0)),
        ];
        let result = sort_by_latency(candidates, &probes, Some("B"), 50);
        assert_eq!(result[0].name, "B");
    }

    #[test]
    fn standby_bypassed_outside_tolerance() {
        // standby=B at 80ms, best=A at 10ms, tolerance=50ms → 80 > 10+50 → A is #1
        let candidates = vec![node("A"), node("B"), node("C")];
        let probes = vec![
            probe("A", Some(10.0)),
            probe("B", Some(80.0)),
            probe("C", Some(15.0)),
        ];
        let result = sort_by_latency(candidates, &probes, Some("B"), 50);
        assert_eq!(result[0].name, "A");
    }

    #[test]
    fn timeout_nodes_sorted_to_end() {
        let candidates = vec![node("A"), node("timeout_node"), node("B")];
        let probes = vec![
            probe("A", Some(50.0)),
            probe("timeout_node", None),
            probe("B", Some(30.0)),
        ];
        let result = sort_by_latency(candidates, &probes, None, 50);
        assert_eq!(result[2].name, "timeout_node");
    }
}
