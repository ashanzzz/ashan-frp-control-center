use crate::state::AppState;
use anyhow::{anyhow, Context, Result};
use ashan_frp_chmlfrp::RemoteTunnel;
use ashan_frp_cloudflare::DnsRecord;
use ashan_frp_domain::{LayerState, LayerStatus, TunnelRow};
use ashan_frp_frpc_runtime::ReadinessError;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc, time::Duration};

pub async fn build_rows(state: &Arc<AppState>) -> Result<Vec<TunnelRow>> {
    let plans = state.db.list_tunnels().await?;
    let remote = if state.chml.configured() {
        state.chml.list_tunnels().await.unwrap_or_default()
    } else {
        vec![]
    };
    let dns = if state.cf.configured() {
        state.cf.list_a_records().await.unwrap_or_default()
    } else {
        vec![]
    };
    let routing = state.db.routing_state().await?;
    let active = routing.active_node.clone().unwrap_or_default();
    let active_ip = if active.is_empty() {
        None
    } else {
        state
            .chml
            .node_info(&active)
            .await
            .ok()
            .map(|node| node.real_ip)
    };
    let runtime = state.frpc.status().await;
    let frpc_states = state.frpc.tunnel_states().await;
    let remote_map: HashMap<String, RemoteTunnel> = remote
        .into_iter()
        .map(|tunnel| (tunnel.tunnel_name.clone(), tunnel))
        .collect();
    let mut dns_map: HashMap<String, Vec<DnsRecord>> = HashMap::new();
    for record in dns {
        dns_map.entry(record.name.clone()).or_default().push(record);
    }

    Ok(plans
        .into_iter()
        .map(|plan| {
            let chmlfrp = match remote_map.get(&plan.name) {
                None => LayerStatus {
                    state: LayerState::Missing,
                    label: "不存在".into(),
                    detail: None,
                },
                Some(remote) if !active.is_empty() && remote.node != active => LayerStatus {
                    state: LayerState::Drift,
                    label: "节点不一致".into(),
                    detail: Some(format!("实际节点: {}", remote.node)),
                },
                Some(remote)
                    if remote.local_ip != plan.local_ip
                        || remote.local_port != plan.local_port
                        || !remote.port_type.eq_ignore_ascii_case(&plan.protocol)
                        || (matches!(plan.protocol.to_ascii_lowercase().as_str(), "http" | "https")
                            && !remote.effective_domain().eq_ignore_ascii_case(&plan.domain)) =>
                {
                    LayerStatus {
                        state: LayerState::Drift,
                        label: "配置不一致".into(),
                        detail: None,
                    }
                }
                Some(_) => LayerStatus::ok("已存在"),
            };

            let frpc = find_frpc_state(&frpc_states, &plan.name)
                .map(|state| state.state.clone())
                .unwrap_or_else(|| fallback_frpc_status(&runtime));

            let cloudflare = if !plan.dns_managed {
                LayerStatus {
                    state: LayerState::Disabled,
                    label: "不管理".into(),
                    detail: None,
                }
            } else {
                match dns_map.get(&plan.domain) {
                    None => LayerStatus {
                        state: LayerState::Missing,
                        label: "不存在".into(),
                        detail: None,
                    },
                    Some(records) if records.len() > 1 => LayerStatus {
                        state: LayerState::Failed,
                        label: "A 记录冲突".into(),
                        detail: Some(format!("发现 {} 条同名 A 记录", records.len())),
                    },
                    Some(records)
                        if active_ip
                            .as_ref()
                            .is_some_and(|ip| records[0].content != *ip) =>
                    {
                        LayerStatus {
                            state: LayerState::Drift,
                            label: "IP 不一致".into(),
                            detail: Some(format!("实际: {}", records[0].content)),
                        }
                    }
                    Some(_) => LayerStatus::ok("已存在"),
                }
            };

            let overall = if matches!(routing.state.as_str(), "failover" | "dns_switching") {
                LayerStatus {
                    state: LayerState::Starting,
                    label: "全局节点切换中".into(),
                    detail: None,
                }
            } else if routing.state == "degraded_dns" {
                LayerStatus {
                    state: LayerState::Drift,
                    label: "全局 DNS 待收敛".into(),
                    detail: None,
                }
            } else {
                overall(&chmlfrp, &frpc, &cloudflare, plan.dns_managed)
            };

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

fn find_frpc_state<'a>(
    states: &'a HashMap<String, ashan_frp_domain::FrpcTunnelState>,
    plan_name: &str,
) -> Option<&'a ashan_frp_domain::FrpcTunnelState> {
    states.get(plan_name).or_else(|| {
        states
            .iter()
            .find(|(actual, _)| actual.ends_with(&format!(".{plan_name}")))
            .map(|(_, state)| state)
    })
}

fn fallback_frpc_status(runtime: &ashan_frp_domain::FrpcRuntimeStatus) -> LayerStatus {
    if !runtime.running {
        return LayerStatus {
            state: LayerState::Failed,
            label: "FRPC 未运行".into(),
            detail: runtime.last_error.clone(),
        };
    }
    if !runtime.connected && runtime.last_error.is_some() {
        return LayerStatus {
            state: LayerState::Failed,
            label: "节点连接异常".into(),
            detail: runtime.last_error.clone(),
        };
    }
    if runtime.connected {
        return LayerStatus {
            state: LayerState::Waiting,
            label: "等待隧道日志".into(),
            detail: None,
        };
    }
    LayerStatus {
        state: LayerState::Starting,
        label: "连接中".into(),
        detail: None,
    }
}

fn overall(
    chmlfrp: &LayerStatus,
    frpc: &LayerStatus,
    cloudflare: &LayerStatus,
    dns_managed: bool,
) -> LayerStatus {
    if chmlfrp.state == LayerState::Missing {
        return LayerStatus {
            state: LayerState::Failed,
            label: "ChmlFrp 未部署".into(),
            detail: None,
        };
    }
    if chmlfrp.state == LayerState::Drift {
        return LayerStatus {
            state: LayerState::Drift,
            label: "ChmlFrp 待同步".into(),
            detail: None,
        };
    }
    if frpc.state == LayerState::Failed {
        return LayerStatus {
            state: LayerState::Failed,
            label: "FRPC 异常".into(),
            detail: frpc.detail.clone(),
        };
    }
    if dns_managed && matches!(cloudflare.state, LayerState::Missing | LayerState::Drift) {
        return LayerStatus {
            state: LayerState::Drift,
            label: "DNS 待同步".into(),
            detail: None,
        };
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
            LayerStatus {
                state: LayerState::Starting,
                label: "等待 FRPC 验证".into(),
                detail: None,
            }
        }
    } else {
        LayerStatus::unknown("检查中")
    }
}

pub async fn reconcile_all(state: &Arc<AppState>, job_id: &str) -> Result<()> {
    let routing = state.db.routing_state().await?;
    let active = routing
        .active_node
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

    // Do not continue to FRPC/DNS until ChmlFrp reflects the complete desired set.
    let verified = state.chml.list_tunnels().await?;
    let verified_map: HashMap<String, RemoteTunnel> = verified
        .into_iter()
        .map(|tunnel| (tunnel.tunnel_name.clone(), tunnel))
        .collect();
    let mut drift = Vec::new();
    for plan in &plans {
        match verified_map.get(&plan.name) {
            Some(remote) if remote.matches_plan_on_node(plan, &active) => {}
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
    if !drift.is_empty() {
        return Err(anyhow!(
            "ChmlFrp reconcile verification failed: {}",
            drift.join("; ")
        ));
    }

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

    match state
        .frpc
        .wait_ready(&names, Duration::from_secs(20))
        .await
    {
        Ok(()) => {}
        Err(ReadinessError::Node(err)) => {
            return Err(anyhow!("active node FRPC runtime failure: {err}"));
        }
        Err(ReadinessError::NonNode(err)) => {
            return Err(anyhow!("FRPC configuration/runtime failure: {err}"));
        }
        Err(ReadinessError::Timeout { connected, missing }) => {
            return Err(anyhow!(
                "FRPC readiness timeout; connected={connected}, missing={missing}"
            ));
        }
    }

    // Cloudflare is always the final execution layer.
    if dns_required {
        let records = state.cf.list_a_records().await?;
        ensure_unique_managed_a_records(&plans, &records)?;
        let records_by_name: HashMap<String, _> = records
            .into_iter()
            .map(|record| (record.name.clone(), record))
            .collect();
        for plan in &plans {
            if !plan.dns_managed {
                continue;
            }
            let existing = records_by_name.get(&plan.domain);
            let record = state
                .cf
                .upsert_a_record(&plan.domain, &node.real_ip, existing)
                .await
                .with_context(|| format!("sync Cloudflare A record {}", plan.domain))?;
            if plan.cloudflare_record_id.as_deref() != Some(record.id.as_str()) {
                state
                    .db
                    .set_cloudflare_record_id(plan.id, &record.id)
                    .await?;
            }
        }
    }

    // Reconcile can also finish a previous DNS-degraded failover.
    if matches!(routing.state.as_str(), "degraded_dns" | "dns_switching") {
        state.db.finalize_active_node().await?;
    }

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

fn ensure_unique_managed_a_records(plans: &[ashan_frp_domain::TunnelPlan], records: &[DnsRecord]) -> Result<()> {
    let mut conflicts = Vec::new();
    for plan in plans.iter().filter(|plan| plan.dns_managed) {
        let count = records.iter().filter(|record| record.name.eq_ignore_ascii_case(&plan.domain)).count();
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
