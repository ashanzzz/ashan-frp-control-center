use ashan_frp_domain::NodeSummary;

pub fn ordered_candidates(
    active: &str,
    preferred_standby: Option<&str>,
    nodes: &[NodeSummary],
) -> Vec<NodeSummary> {
    let mut out = Vec::new();
    if let Some(name) = preferred_standby {
        if let Some(node) = nodes.iter().find(|n| n.name == name && eligible(n, active)) {
            out.push(node.clone());
        }
    }
    for node in nodes {
        if eligible(node, active) && !out.iter().any(|n| n.name == node.name) {
            out.push(node.clone());
        }
    }
    out
}

pub fn choose_target(
    active: &str,
    preferred_standby: Option<&str>,
    nodes: &[NodeSummary],
) -> Option<NodeSummary> {
    ordered_candidates(active, preferred_standby, nodes)
        .into_iter()
        .next()
}

fn eligible(node: &NodeSummary, active: &str) -> bool {
    node.name != active
        && node.web_supported
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
        assert!(choose_target("A", None, &[node("A")]).is_none());
    }

    #[test]
    fn standby_is_first() {
        let nodes = vec![node("C"), node("B")];
        let result = ordered_candidates("A", Some("B"), &nodes);
        assert_eq!(result[0].name, "B");
    }
}
