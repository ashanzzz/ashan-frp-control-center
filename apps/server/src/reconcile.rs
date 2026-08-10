use crate::state::AppState;
use anyhow::{Context, Result, anyhow};
use ashan_frp_chmlfrp::RemoteTunnel;
use ashan_frp_cloudflare::DnsRecord;
use ashan_frp_domain::{
    FrpcRuntimeStatus, FrpcTunnelState, LayerState, LayerStatus, RoutingPhase, TunnelPlan,
    TunnelRow,
};
use ashan_frp_frpc_runtime::ReadinessError;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc, time::Duration};

pub async fn build_rows(state: &Arc<AppState>) -> Result<Vec<TunnelRow>> {
    let plans = state.db.list_tunnels().await?;
    let chmlfrp_configured = state.chml.configured();
    let (remote_tunnels, chmlfrp_available) = if chmlfrp_configured {
        match state.chml.list_tunnels().await {
            Ok(tunnels) => (tunnels, true),
            Err(_) => (Vec::new(), false),
        }
    } else {
        (Vec::new(), false)
    };

    let cloudflare_configured = state.cf.configured();
    let (dns_records, cloudflare_available) = if cloudflare_configured {
        match state.cf.list_a_records().await {
            Ok(records) => (records, true),
            Err(_) => (Vec::new(), false),
        }
    } else {
        (Vec::new(), false)
    };

    let routing = state.db.routing_state().await?;
    let active = routing.active_node.clone().unwrap_or_default();
    let active_ip = if chmlfrp_available && !active.is_empty() {
        state
            .chml
            .node_info(&active)
            .await
            .ok()
            .map(|node| node.real_ip)
    } else {
        None
    };

    let runtime = state.frpc.status().await;
    let frpc_states = state.frpc.tunnel_states().await;
    let remote_by_name: HashMap<String, RemoteTunnel> = remote_tunnels
        .into_iter()
        .map(|tunnel| (tunnel.tunnel_name.clone(), tunnel))
        .collect();
    let mut dns_by_name: HashMap<String, Vec<DnsRecord>> = HashMap::new();
    for record in dns_records {
        dns_by_name
            .entry(record.name.to_ascii_lowercase())
            .or_default()
            .push(record);
    }

    Ok(plans
        .into_iter()
        .map(|plan| {
            let chmlfrp = chmlfrp_status(
                &plan,
                chmlfrp_configured,
                chmlfrp_available,
                &active,
                &remote_by_name,
            );
            let frpc = find_frpc_state(&frpc_states, &plan.name)
                .map(|state| state.state.clone())
                .unwrap_or_else(|| fallback_frpc_status(&runtime));
            let cloudflare = cloudflare_status(
                &plan,
                cloudflare_configured,
                cloudflare_available,
                active_ip.as_deref(),
                &dns_by_name,
            );
            let overall = overall_status(
                routing.state,
                &chmlfrp,
                &frpc,
                &cloudflare,
                plan.dns_managed,
            );

            TunnelRow {
                plan,
                chmlfrp,
                frpc,
                cloudflare,
                overall,
            }
        })
        .collect())
}

fn chmlfrp_status(
    plan: &TunnelPlan,
    configured: bool,
    available: bool,
    active_node: &str,
    remote_by_name: &HashMap<String, RemoteTunnel>,
) -> LayerStatus {
    if !configured {
        return layer_status(LayerState::Disabled, "未配置", None);
    }
    if !available {
        return layer_status(LayerState::Unknown, "API 不可用", None);
    }

    let Some(remote) = remote_by_name.get(&plan.name) else {
        return layer_status(LayerState::Missing, "不存在", None);
    };

    if !active_node.is_empty() && remote.node != active_node {
        return layer_status(
            LayerState::Drift,
            "节点不一致",
            Some(format!("实际节点: {}", remote.node)),
        );
    }

    let is_web = matches!(
        plan.protocol.to_ascii_lowercase().as_str(),
        "http" | "https"
    );
    let configuration_drift = remote.local_ip != plan.local_ip
        || remote.local_port != plan.local_port
        || !remote.port_type.eq_ignore_ascii_case(&plan.protocol)
        || (is_web && !remote.effective_domain().eq_ignore_ascii_case(&plan.domain));

    if configuration_drift {
        layer_status(LayerState::Drift, "配置不一致", None)
    } else {
        LayerStatus::ok("已存在")
    }
}

fn cloudflare_status(
    plan: &TunnelPlan,
    configured: bool,
    available: bool,
    active_ip: Option<&str>,
    records_by_name: &HashMap<String, Vec<DnsRecord>>,
) -> LayerStatus {
    if !plan.dns_managed {
        return layer_status(LayerState::Disabled, "不管理", None);
    }
    if !configured {
        return layer_status(LayerState::Disabled, "未配置", None);
    }
    if !available {
        return layer_status(LayerState::Unknown, "API 不可用", None);
    }

    let key = plan.domain.to_ascii_lowercase();
    let Some(records) = records_by_name.get(&key) else {
        return layer_status(LayerState::Missing, "不存在", None);
    };
    if records.len() > 1 {
        return layer_status(
            LayerState::Failed,
            "A 记录冲突",
            Some(format!("发现 {} 条同名 A 记录", records.len())),
        );
    }

    if let Some(expected_ip) = active_ip
        && records[0].content != expected_ip
    {
        return layer_status(
            LayerState::Drift,
            "IP 不一致",
            Some(format!("实际: {}", records[0].content)),
        );
    }

    LayerStatus::ok("已存在")
}

fn find_frpc_state<'a>(
    states: &'a HashMap<String, FrpcTunnelState>,
    plan_name: &str,
) -> Option<&'a FrpcTunnelState> {
    states.get(plan_name).or_else(|| {
        states
            .iter()
            .find(|(actual, _)| actual.ends_with(&format!(".{plan_name}")))
            .map(|(_, state)| state)
    })
}

fn fallback_frpc_status(runtime: &FrpcRuntimeStatus) -> LayerStatus {
    if !runtime.running {
        return layer_status(
            LayerState::Failed,
            "FRPC 未运行",
            runtime.last_error.clone(),
        );
    }
    if !runtime.connected && runtime.last_error.is_some() {
        return layer_status(
            LayerState::Failed,
            "节点连接异常",
            runtime.last_error.clone(),
        );
    }
    if runtime.connected {
        return layer_status(LayerState::Waiting, "等待隧道日志", None);
    }
    layer_status(LayerState::Starting, "连接中", None)
}

fn overall_status(
    phase: RoutingPhase,
    chmlfrp: &LayerStatus,
    frpc: &LayerStatus,
    cloudflare: &LayerStatus,
    dns_managed: bool,
) -> LayerStatus {
    if matches!(phase, RoutingPhase::Failover | RoutingPhase::DnsSwitching) {
        return layer_status(LayerState::Starting, "全局节点切换中", None);
    }
    if phase == RoutingPhase::DegradedDns {
        return layer_status(LayerState::Drift, "全局 DNS 待收敛", None);
    }
    if phase == RoutingPhase::Failed {
        return layer_status(LayerState::Failed, "全局状态异常", None);
    }

    match chmlfrp.state {
        LayerState::Missing => {
            return layer_status(LayerState::Failed, "ChmlFrp 未部署", None);
        }
        LayerState::Drift => {
            return layer_status(LayerState::Drift, "ChmlFrp 待同步", None);
        }
        LayerState::Unknown => {
            return layer_status(LayerState::Unknown, "ChmlFrp API 不可用", None);
        }
        LayerState::Disabled => {
            return layer_status(LayerState::Disabled, "ChmlFrp 未配置", None);
        }
        _ => {}
    }

    if frpc.state == LayerState::Failed {
        return layer_status(LayerState::Failed, "FRPC 异常", frpc.detail.clone());
    }

    if dns_managed {
        match cloudflare.state {
            LayerState::Failed => {
                return layer_status(LayerState::Failed, "DNS 异常", cloudflare.detail.clone());
            }
            LayerState::Missing | LayerState::Drift => {
                return layer_status(LayerState::Drift, "DNS 待同步", None);
            }
            LayerState::Unknown => {
                return layer_status(LayerState::Unknown, "Cloudflare API 不可用", None);
            }
            LayerState::Disabled => {
                return layer_status(LayerState::Disabled, "Cloudflare 未配置", None);
            }
            _ => {}
        }
    }

    if chmlfrp.state == LayerState::Ok
        && matches!(
            frpc.state,
            LayerState::Ok | LayerState::Waiting | LayerState::Starting
        )
        && (!dns_managed || cloudflare.state == LayerState::Ok)
    {
        if frpc.state == LayerState::Ok {
            LayerStatus::ok("正常")
        } else {
            layer_status(LayerState::Starting, "等待 FRPC 验证", None)
        }
    } else {
        LayerStatus::unknown("检查中")
    }
}

fn layer_status(state: LayerState, label: &str, detail: Option<String>) -> LayerStatus {
    LayerStatus {
        state,
        label: label.to_owned(),
        detail,
    }
}

pub async fn reconcile_all(state: &Arc<AppState>, job_id: &str) -> Result<()> {
    let routing = state.db.routing_state().await?;
    let active = routing
        .active_node
        .clone()
        .ok_or_else(|| anyhow!("未配置全局活动节点"))?;
    let plans = state
        .db
        .list_tunnels()
        .await?
        .into_iter()
        .filter(|plan| plan.enabled)
        .collect::<Vec<_>>();
    if plans.is_empty() {
        return Err(anyhow!("没有启用的计划隧道"));
    }

    let dns_required = plans.iter().any(|plan| plan.dns_managed);
    if dns_required && !state.cf.configured() {
        return Err(anyhow!(
            "reconcile preflight failed: managed DNS exists but Cloudflare is not configured"
        ));
    }
    if dns_required {
        let records = state
            .cf
            .list_a_records()
            .await
            .context("reconcile preflight: read Cloudflare A records")?;
        ensure_unique_managed_a_records(&plans, &records)?;
    }

    let node = state
        .chml
        .node_info(&active)
        .await
        .context("reconcile preflight: read active ChmlFrp node")?;
    if !node.state.eq_ignore_ascii_case("online") {
        return Err(anyhow!(
            "active ChmlFrp node is not online: state={}",
            node.state
        ));
    }
    if dns_required && node.real_ip.trim().is_empty() {
        return Err(anyhow!("active ChmlFrp node has no realIp"));
    }

    state
        .db
        .activity(
            Some(job_id),
            "reconcile",
            "info",
            "开始全局一致性同步；所有计划隧道统一使用当前活动节点",
            None,
            Some(&active),
            serde_json::json!({"tunnels": plans.len()}),
        )
        .await?;

    let mut remote = state.chml.list_tunnels().await?;
    for plan in &plans {
        if let Some(existing) = remote
            .iter()
            .find(|tunnel| tunnel.tunnel_name == plan.name)
            .cloned()
        {
            if !existing.matches_plan_on_node(plan, &active) {
                state
                    .chml
                    .sync_tunnel(&existing, plan, &active)
                    .await
                    .with_context(|| format!("sync tunnel {} to active node", plan.name))?;
            }
        } else {
            let created = state
                .chml
                .create_tunnel(plan, &active)
                .await
                .with_context(|| format!("create missing tunnel {}", plan.name))?;
            remote.push(created);
        }
    }

    verify_chmlfrp_desired_state(state, &plans, &active).await?;

    let names = plans
        .iter()
        .map(|plan| plan.name.clone())
        .collect::<Vec<_>>();
    let config = state.chml.generated_config(&active, &names).await?;
    if ashan_frp_frpc_log::config_has_duplicate_proxy_names(&config) {
        return Err(anyhow!(
            "ChmlFrp generated config contains duplicate proxy names"
        ));
    }

    let config_unchanged = state.frpc.config_matches(&config).await?;
    let runtime = state.frpc.status().await;
    if config_unchanged && runtime.running && runtime.connected {
        state
            .db
            .activity(
                Some(job_id),
                "reconcile",
                "info",
                "FRPC 配置未变化且进程运行中，跳过不必要的重启",
                None,
                Some(&active),
                serde_json::json!({"generation": state.frpc.generation()}),
            )
            .await?;
    } else {
        let sha = hex::encode(Sha256::digest(config.as_bytes()));
        let revision = state
            .db
            .save_config_revision(&active, &sha, &config)
            .await?;
        state.frpc.write_config(&config, revision).await?;
        state
            .frpc
            .restart()
            .await
            .context("restart frpc with ChmlFrp generated config")?;
    }

    match state.frpc.wait_ready(&names, Duration::from_secs(20)).await {
        Ok(()) => {}
        Err(ReadinessError::Node(error)) => {
            return Err(anyhow!("active node FRPC runtime failure: {error}"));
        }
        Err(ReadinessError::NonNode(error)) => {
            return Err(anyhow!("FRPC configuration/runtime failure: {error}"));
        }
        Err(ReadinessError::Timeout { connected, missing }) => {
            return Err(anyhow!(
                "FRPC readiness timeout; connected={connected}, missing={missing}"
            ));
        }
    }

    // Cloudflare is always the final execution layer.
    if dns_required {
        reconcile_dns(state, &plans, &node.real_ip).await?;
    }

    // A successful reconcile proves that ChmlFrp, FRPC and managed DNS have
    // converged again. Clear any recoverable failed/degraded phase.
    state.db.finalize_active_node().await?;

    state
        .db
        .activity(
            Some(job_id),
            "reconcile",
            "info",
            "全局一致性同步完成",
            None,
            Some(&active),
            serde_json::json!({"tunnels": plans.len()}),
        )
        .await?;
    Ok(())
}

async fn verify_chmlfrp_desired_state(
    state: &Arc<AppState>,
    plans: &[TunnelPlan],
    active: &str,
) -> Result<()> {
    let verified = state.chml.list_tunnels().await?;
    let verified_by_name: HashMap<String, RemoteTunnel> = verified
        .into_iter()
        .map(|tunnel| (tunnel.tunnel_name.clone(), tunnel))
        .collect();
    let mut drift = Vec::new();

    for plan in plans {
        match verified_by_name.get(&plan.name) {
            Some(remote) if remote.matches_plan_on_node(plan, active) => {}
            Some(remote) => drift.push(format!(
                "{}: node={} local={}:{} protocol={} domain={}",
                plan.name,
                remote.node,
                remote.local_ip,
                remote.local_port,
                remote.port_type,
                remote.effective_domain()
            )),
            None => drift.push(format!("{}: missing after reconcile", plan.name)),
        }
    }

    if drift.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "ChmlFrp reconcile verification failed: {}",
            drift.join("; ")
        ))
    }
}

async fn reconcile_dns(state: &Arc<AppState>, plans: &[TunnelPlan], active_ip: &str) -> Result<()> {
    let records = state.cf.list_a_records().await?;
    ensure_unique_managed_a_records(plans, &records)?;
    let records_by_name: HashMap<String, DnsRecord> = records
        .into_iter()
        .map(|record| (record.name.to_ascii_lowercase(), record))
        .collect();

    for plan in plans.iter().filter(|plan| plan.dns_managed) {
        let key = plan.domain.to_ascii_lowercase();
        let existing = records_by_name.get(&key);
        let record = state
            .cf
            .upsert_a_record(&plan.domain, active_ip, existing)
            .await
            .with_context(|| format!("sync Cloudflare A record {}", plan.domain))?;
        if plan.cloudflare_record_id.as_deref() != Some(record.id.as_str()) {
            state
                .db
                .set_cloudflare_record_id(plan.id, &record.id)
                .await?;
        }
    }
    Ok(())
}

fn ensure_unique_managed_a_records(plans: &[TunnelPlan], records: &[DnsRecord]) -> Result<()> {
    let mut conflicts = Vec::new();
    for plan in plans.iter().filter(|plan| plan.dns_managed) {
        let count = records
            .iter()
            .filter(|record| record.name.eq_ignore_ascii_case(&plan.domain))
            .count();
        if count > 1 {
            conflicts.push(format!("{} ({count} A records)", plan.domain));
        }
    }
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "Cloudflare managed A-record conflict: {}; exactly zero or one A record is allowed per managed domain",
            conflicts.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{layer_status, overall_status};
    use ashan_frp_domain::{LayerState, LayerStatus, RoutingPhase};

    #[test]
    fn dns_conflict_is_an_overall_failure() {
        let status = overall_status(
            RoutingPhase::Idle,
            &LayerStatus::ok("已存在"),
            &LayerStatus::ok("正常"),
            &layer_status(LayerState::Failed, "A 记录冲突", None),
            true,
        );
        assert_eq!(status.state, LayerState::Failed);
    }

    #[test]
    fn provider_outage_is_not_reported_as_missing_resource() {
        let status = overall_status(
            RoutingPhase::Idle,
            &layer_status(LayerState::Unknown, "API 不可用", None),
            &LayerStatus::ok("正常"),
            &LayerStatus::ok("已存在"),
            true,
        );
        assert_eq!(status.state, LayerState::Unknown);
        assert_eq!(status.label, "ChmlFrp API 不可用");
    }
}
