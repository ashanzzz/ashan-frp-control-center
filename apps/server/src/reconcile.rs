use crate::state::AppState;
use anyhow::{anyhow, Context, Result};
use ashan_frp_chmlfrp::RemoteTunnel;
use ashan_frp_domain::{LayerState, LayerStatus, TunnelRow};
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
    let dns_map: HashMap<String, _> = dns
        .into_iter()
        .map(|record| (record.name.clone(), record))
        .collect();

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
                        || !remote.port_type.eq_ignore_ascii_case(&plan.protocol) =>
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
                    Some(record)
                        if active_ip
                            .as_ref()
                            .is_some_and(|ip| record.content != *ip) =>
                    {
                        LayerStatus {
                            state: LayerState::Drift,
                            label: "IP 不一致".into(),
                            detail: Some(format!("实际: {}", record.content)),
                        }
                    }
                    Some(_) => LayerStatus::ok("已存在"),
                }
            };

            let overall = if routing.state == "failover" {
                LayerStatus {
                    state: LayerState::Starting,
                    label: "全局节点切换中".into(),
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
    state
        .db
        .activity(
            Some(job_id),
            "reconcile",
            "info",
            "读取 ChmlFrp 隧道",
            None,
            Some(&active),
            serde_json::json!({}),
        )
        .await?;

    let plans = state
        .db
        .list_tunnels()
        .await?
        .into_iter()
        .filter(|plan| plan.enabled)
        .collect::<Vec<_>>();
    let mut remote = state.chml.list_tunnels().await?;

    // The normal reconcile path also enforces one global node. It never chooses a node per tunnel.
    for plan in &plans {
        if let Some(existing) = remote
            .iter()
            .find(|tunnel| tunnel.tunnel_name == plan.name)
            .cloned()
        {
            if existing.node != active
                || existing.local_ip != plan.local_ip
                || existing.local_port != plan.local_port
                || !existing.port_type.eq_ignore_ascii_case(&plan.protocol)
            {
                state.chml.sync_tunnel(&existing, plan, &active).await?;
            }
        } else {
            let created = state.chml.create_tunnel(plan, &active).await?;
            remote.push(created);
        }
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
    state
        .frpc
        .wait_ready(&names, Duration::from_secs(20))
        .await
        .context("validate all tunnels from FRPC logs")?;

    // Cloudflare is always the final execution layer.
    if state.cf.configured() {
        let node = state.chml.node_info(&active).await?;
        if node.real_ip.trim().is_empty() {
            return Err(anyhow!("active ChmlFrp node has no realIp"));
        }
        let records = state.cf.list_a_records().await?;
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
                .upsert_a_record(
                    &plan.domain,
                    &node.real_ip,
                    existing,
                )
                .await?;
            if plan.cloudflare_record_id.as_deref() != Some(record.id.as_str()) {
                state
                    .db
                    .set_cloudflare_record_id(plan.id, &record.id)
                    .await?;
            }
        }
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
