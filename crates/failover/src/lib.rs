use ashan_frp_domain::NodeSummary;

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
}
