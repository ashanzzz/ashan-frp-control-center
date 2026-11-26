use ashan_frp_domain::NodeSummary;

/// A latency probe result for one node.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub node_name: String,
    /// Average RTT in milliseconds; None means all probe attempts failed.
    pub avg_rtt_ms: Option<f64>,
}

/// Candidate selection policy for node filtering and recommendations.
#[derive(Debug, Clone)]
pub struct CandidatePolicy {
    pub require_web: bool,
    pub require_high_bandwidth: bool,
    pub exclude_icp_required: bool,
    pub exclude_whitelist_and_ban_overseas: bool,
    pub max_load1: f64,
}

impl Default for CandidatePolicy {
    fn default() -> Self {
        Self {
            require_web: false,
            require_high_bandwidth: true,
            exclude_icp_required: false,
            exclude_whitelist_and_ban_overseas: true,
            max_load1: 5.0,
        }
    }
}

/// Human-readable exclusion reasons for a node under `policy`. Empty means eligible.
pub fn ineligible_reasons(node: &NodeSummary, policy: &CandidatePolicy) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if !node.state.eq_ignore_ascii_case("online") {
        reasons.push("节点离线");
    }
    if node.quarantined_until.is_some() {
        reasons.push("故障隔离中");
    }
    if node.policy.attacked {
        reasons.push("被攻击/暂时不可用");
    }
    if policy.require_web && !node.web_supported {
        reasons.push("不支持建站");
    }
    if policy.require_high_bandwidth && node.policy.forbids_drive {
        reasons.push("禁止网盘/大流量");
    }
    if policy.require_high_bandwidth && node.policy.bandwidth_limited {
        reasons.push("高峰限速/线路较差");
    }
    if policy.exclude_icp_required && node.policy.requires_icp {
        reasons.push("需域名备案");
    }
    if policy.exclude_whitelist_and_ban_overseas && node.policy.requires_whitelist {
        reasons.push("需加群人工过白");
    }
    if policy.exclude_whitelist_and_ban_overseas && node.policy.bans_overseas {
        reasons.push("封禁海外IP(影响Cloudflare)");
    }
    if node.load1.is_some_and(|l| l > policy.max_load1) {
        reasons.push("负载过高");
    }
    reasons
}

pub fn ordered_candidates(
    active: &str,
    preferred_standby: Option<&str>,
    nodes: &[NodeSummary],
    require_web: bool,
) -> Vec<NodeSummary> {
    let policy = CandidatePolicy {
        require_web,
        require_high_bandwidth: false,
        exclude_icp_required: false,
        exclude_whitelist_and_ban_overseas: false,
        max_load1: f64::INFINITY,
    };
    ordered_candidates_with_policy(active, preferred_standby, nodes, &policy)
}

pub fn ordered_candidates_with_policy(
    active: &str,
    preferred_standby: Option<&str>,
    nodes: &[NodeSummary],
    policy: &CandidatePolicy,
) -> Vec<NodeSummary> {
    let mut out = Vec::new();
    if let Some(name) = preferred_standby
        && let Some(node) = nodes
            .iter()
            .find(|node| node.name == name && eligible_with_policy(node, active, policy))
    {
        out.push(node.clone());
    }
    for node in nodes {
        if eligible_with_policy(node, active, policy) && !out.iter().any(|n| n.name == node.name) {
            out.push(node.clone());
        }
    }
    out
}

/// Sort `candidates` by ascending probe RTT with STANDBY priority protection.
///
/// If the preferred standby node's RTT is within `tolerance_ms` of the best RTT,
/// it is kept at position 0. Otherwise, nodes are ordered by ascending RTT (breaking
/// ties by big-bandwidth tag, defense availability, and lower load1).
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
            .then_with(|| b.policy.high_bandwidth.cmp(&a.policy.high_bandwidth))
            .then_with(|| b.fangyu.unwrap_or(false).cmp(&a.fangyu.unwrap_or(false)))
            .then_with(|| {
                a.load1
                    .unwrap_or(f64::INFINITY)
                    .partial_cmp(&b.load1.unwrap_or(f64::INFINITY))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
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

fn eligible_with_policy(node: &NodeSummary, active: &str, policy: &CandidatePolicy) -> bool {
    node.name != active && ineligible_reasons(node, policy).is_empty()
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
            frps_port: Some(7000),
            nodegroup: Some("vip".into()),
            china: Some(true),
            fangyu: Some(true),
            udp: Some(true),
            toowhite: Some(false),
            load1: Some(0.2),
            load5: None,
            load15: None,
            bandwidth_usage_percent: Some(1.0),
            notes: String::new(),
            tags: Vec::new(),
            policy: Default::default(),
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

    #[test]
    fn policy_filters_attacked_forbids_drive_high_load_and_whitelist() {
        let mut attacked = node("北京多线");
        attacked.notes = "节点被攻击，暂时无法使用。建站需备案接入腾讯云".into();
        attacked.classify();

        let mut sy = node("沈阳电信");
        sy.notes = "此节点禁止大流量（例如MC联机、网盘、长期大文件传输）".into();
        sy.classify();

        let mut cq = node("重庆联通");
        cq.load1 = Some(13.16);
        cq.classify();

        let mut qz = node("福建泉州电信");
        qz.notes = "建站需加入交流群提交域名过白，节点ban海外".into();
        qz.classify();

        let mut cd = node("成都多线-2");
        cd.notes = "建站需备案接入腾讯云".into();
        cd.classify();

        let mut la3 = node("美国洛杉矶-3");
        la3.notes = "超大带宽节点，AS4837线路。强烈建议文件传输使用".into();
        la3.china = Some(false);
        la3.classify();

        let policy = CandidatePolicy {
            require_web: true,
            require_high_bandwidth: true,
            exclude_icp_required: false,
            exclude_whitelist_and_ban_overseas: true,
            max_load1: 5.0,
        };
        let list = vec![attacked, sy, cq, qz, cd.clone(), la3.clone()];
        let out = ordered_candidates_with_policy("旧节点", None, &list, &policy);
        let names: Vec<_> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["成都多线-2", "美国洛杉矶-3"]);

        // If ICP exclusion is enabled, 成都多线-2 is excluded too.
        let strict_icp = CandidatePolicy {
            exclude_icp_required: true,
            ..policy
        };
        let out_no_icp = ordered_candidates_with_policy("旧节点", None, &list, &strict_icp);
        assert_eq!(
            out_no_icp
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            vec!["美国洛杉矶-3"]
        );
    }
}
