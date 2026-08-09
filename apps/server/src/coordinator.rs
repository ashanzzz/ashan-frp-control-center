use crate::{reconcile, state::AppState};
use anyhow::{anyhow, Result};
use ashan_frp_domain::{FaultDomain, FrpcEvent, FrpcEventType};
use ashan_frp_failover::ordered_candidates;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct Coordinator {
    state: Arc<AppState>,
    lock: Arc<Mutex<()>>,
}

impl Coordinator {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn spawn_frpc_fault_watcher(&self) {
        let mut rx = self.state.frpc.subscribe();
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) if event.triggers_failover => {
                        let next = me.clone();
                        tokio::spawn(async move {
                            if let Err(err) = next.global_failover(Some(event)).await {
                                error!(error = %err, "automatic global failover failed");
                            }
                        });
                    }
                    Ok(event) if event.event_type == FrpcEventType::NetworkAmbiguous => {
                        // A bare timeout may be the user's own WAN. Promote it to a node fault
                        // only when ChmlFrp itself confirms the current active node is offline.
                        let next = me.clone();
                        tokio::spawn(async move {
                            if let Ok(Some(confirmed)) = next.confirm_ambiguous_network_fault(event).await {
                                if let Err(err) = next.global_failover(Some(confirmed)).await {
                                    error!(error = %err, "confirmed network fault failover failed");
                                }
                            }
                        });
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lost = n, "FRPC event receiver lagged");
                    }
                    Err(_) => break,
                }
            }
        });
    }


    async fn confirm_ambiguous_network_fault(&self, mut event: FrpcEvent) -> Result<Option<FrpcEvent>> {
        let routing = self.state.db.routing_state().await?;
        let Some(active) = routing.active_node else { return Ok(None); };
        let info = match self.state.chml.node_info(&active).await {
            Ok(info) => info,
            Err(_) => return Ok(None),
        };
        if !info.state.eq_ignore_ascii_case("online") {
            event.event_type = FrpcEventType::ServerConnectionFailure;
            event.fault_domain = FaultDomain::Node;
            event.severity = "critical".into();
            event.triggers_failover = true;
            self.state.db.activity(
                None,
                "frpc_fault",
                "critical",
                "FRPC 模糊网络故障经 ChmlFrp 节点状态确认，提升为节点级故障",
                event.proxy_name.as_deref(),
                Some(&active),
                serde_json::json!({"raw": event.raw, "node_state": info.state}),
            ).await?;
            return Ok(Some(event));
        }
        Ok(None)
    }

    pub async fn global_failover(&self, trigger: Option<FrpcEvent>) -> Result<String> {
        let _guard = self
            .lock
            .try_lock()
            .map_err(|_| anyhow!("GLOBAL_FAILOVER already running"))?;
        let job = Uuid::new_v4().to_string();
        *self.state.failover_job.write().await = Some(job.clone());

        let result = self.run_global_failover(&job, trigger).await;
        if let Err(err) = &result {
            let _ = self.state.db.set_routing_state("failed").await;
            let _ = self
                .state
                .db
                .activity(
                    Some(&job),
                    "failover",
                    "error",
                    "全局故障切换失败",
                    None,
                    None,
                    serde_json::json!({"error": err.to_string()}),
                )
                .await;
        }
        *self.state.failover_job.write().await = None;
        result.map(|_| job)
    }

    async fn run_global_failover(&self, job: &str, trigger: Option<FrpcEvent>) -> Result<()> {
        let routing = self.state.db.routing_state().await?;
        if !routing.failover_enabled && trigger.is_some() {
            return Err(anyhow!("automatic failover disabled"));
        }
        let old = routing
            .active_node
            .clone()
            .ok_or_else(|| anyhow!("no active node configured"))?;
        self.state.db.set_routing_state("failover").await?;

        let reason = trigger
            .as_ref()
            .map(|e| format!("{:?}", e.event_type))
            .unwrap_or_else(|| "MANUAL_FAILOVER".into());
        let trigger_tunnel = trigger.as_ref().and_then(|e| e.proxy_name.as_deref());
        if trigger.is_some() {
            let old_info = self.state.chml.node_info(&old).await.ok();
            self.state
                .db
                .quarantine_node(
                    &old,
                    old_info.as_ref().map(|n| n.real_ip.as_str()),
                    &reason,
                    trigger_tunnel,
                    routing.quarantine_days,
                )
                .await?;
            self.state
                .db
                .activity(
                    Some(job),
                    "failover",
                    "critical",
                    "确认节点级故障，旧活动节点进入隔离并启动全局切换",
                    trigger_tunnel,
                    Some(&old),
                    serde_json::json!({"reason": reason}),
                )
                .await?;
        } else {
            self.state
                .db
                .activity(
                    Some(job),
                    "failover",
                    "warning",
                    "人工触发全局切换；旧节点不进入故障隔离",
                    None,
                    Some(&old),
                    serde_json::json!({"reason": reason}),
                )
                .await?;
        }

        let mut nodes = self.state.chml.list_nodes().await?;
        for node in &mut nodes {
            node.quarantined_until = self.state.db.quarantine_until(&node.name).await?;
        }
        let candidates = ordered_candidates(&old, routing.standby_node.as_deref(), &nodes);
        if candidates.is_empty() {
            return Err(anyhow!("no eligible standby node"));
        }

        let mut last_error: Option<anyhow::Error> = None;
        for (idx, target) in candidates.iter().enumerate() {
            self.state
                .db
                .activity(
                    Some(job),
                    "failover",
                    "info",
                    "选择全局候选节点，尝试整体迁移",
                    None,
                    Some(&target.name),
                    serde_json::json!({"ip": target.real_ip.clone(), "attempt": idx + 1}),
                )
                .await?;

            match self.migrate_to(job, &target.name).await {
                Ok(()) => {
                    let next_standby = if trigger.is_none() {
                        // A manual switch is not evidence that the old active node is bad;
                        // make it the new preferred standby so manual A<->B switching is reversible.
                        Some(old.as_str())
                    } else {
                        candidates
                            .iter()
                            .skip(idx + 1)
                            .find(|candidate| candidate.name != target.name)
                            .map(|candidate| candidate.name.as_str())
                    };
                    self.state
                        .db
                        .commit_active_node(&target.name, next_standby)
                        .await?;
                    self.state
                        .db
                        .activity(
                            Some(job),
                            "failover",
                            "info",
                            "全局故障切换完成",
                            None,
                            Some(&target.name),
                            serde_json::json!({"all_managed_tunnels": true}),
                        )
                        .await?;
                    info!(job_id = %job, target = %target.name, "global failover complete");
                    return Ok(());
                }
                Err(err) => {
                    // Only an error proven after loading the target node's FRPC configuration
                    // is evidence that the candidate node itself is bad. Provider/API/config
                    // errors must not poison every candidate by quarantining them one by one.
                    if is_target_node_failure(&err) {
                        let target_ip = target.real_ip.as_deref();
                        self.state
                            .db
                            .quarantine_node(
                                &target.name,
                                target_ip,
                                "FORWARD_RECOVERY_TARGET_FAILED",
                                trigger_tunnel,
                                routing.quarantine_days,
                            )
                            .await?;
                        self.state
                            .db
                            .activity(
                                Some(job),
                                "failover",
                                "error",
                                "候选节点运行验证失败；隔离该节点并继续 Forward Recovery",
                                None,
                                Some(&target.name),
                                serde_json::json!({"error": err.to_string()}),
                            )
                            .await?;
                        last_error = Some(err);
                        continue;
                    }

                    self.state
                        .db
                        .activity(
                            Some(job),
                            "failover",
                            "error",
                            "全局迁移在 Provider/配置阶段失败；未误判候选节点故障",
                            None,
                            Some(&target.name),
                            serde_json::json!({"error": err.to_string()}),
                        )
                        .await?;
                    return Err(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("all failover candidates failed")))
    }

    async fn migrate_to(&self, job: &str, target: &str) -> Result<()> {
        let plans = self
            .state
            .db
            .list_tunnels()
            .await?
            .into_iter()
            .filter(|p| p.enabled)
            .collect::<Vec<_>>();
        if plans.is_empty() {
            return Err(anyhow!("no enabled managed tunnels"));
        }

        let remote = self.state.chml.list_tunnels().await?;
        let map: HashMap<String, _> = remote
            .into_iter()
            .map(|t| (t.tunnel_name.clone(), t))
            .collect();
        self.state
            .db
            .activity(
                Some(job),
                "failover",
                "info",
                "ChmlFrp：开始统一迁移全部隧道",
                None,
                Some(target),
                serde_json::json!({"count": plans.len()}),
            )
            .await?;

        // Intentionally one global operation: every enabled plan must end at the
        // same target node. Missing provider objects are created from the plan;
        // existing objects are reconciled from the plan instead of copying drift.
        for plan in &plans {
            if let Some(remote) = map.get(&plan.name) {
                self.state.chml.sync_tunnel(remote, plan, target).await?;
            } else {
                self.state.chml.create_tunnel(plan, target).await?;
            }
        }

        let names = plans.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        let config = self.state.chml.generated_config(target, &names).await?;
        if ashan_frp_frpc_log::config_has_duplicate_proxy_names(&config) {
            return Err(anyhow!(
                "ChmlFrp generated config contains duplicate proxy names"
            ));
        }
        let sha = hex::encode(Sha256::digest(config.as_bytes()));
        let revision = self
            .state
            .db
            .save_config_revision(target, &sha, &config)
            .await?;
        self.state.frpc.write_config(&config, revision).await?;
        self.state.frpc.restart().await?;
        self.state
            .db
            .activity(
                Some(job),
                "failover",
                "info",
                "FRPC：已加载目标节点完整配置，等待运行验证",
                None,
                Some(target),
                serde_json::json!({"revision": revision}),
            )
            .await?;

        self.state
            .frpc
            .wait_ready(&names, Duration::from_secs(20))
            .await
            .map_err(|err| anyhow!("NODE_TARGET_FAILURE: {err}"))?;

        // DNS is deliberately last. No Cloudflare write happens before the new FRPC process
        // is connected and every planned tunnel has emitted a successful startup event.
        let node = self
            .state
            .chml
            .node_info(target)
            .await
            .map_err(|err| anyhow!("NODE_TARGET_FAILURE: node info failed: {err}"))?;
        if node.real_ip.trim().is_empty() {
            return Err(anyhow!("NODE_TARGET_FAILURE: target ChmlFrp node has no realIp"));
        }
        let records = if self.state.cf.configured() {
            self.state.cf.list_a_records().await?
        } else {
            vec![]
        };
        let dns_map: HashMap<String, _> = records
            .into_iter()
            .map(|record| (record.name.clone(), record))
            .collect();

        if self.state.cf.configured() {
            self.state
                .db
                .activity(
                    Some(job),
                    "failover",
                    "info",
                    "Cloudflare：FRPC 新链路验证后统一更新全部受管 A 记录",
                    None,
                    Some(target),
                    serde_json::json!({"ip": node.real_ip.clone()}),
                )
                .await?;
            for plan in &plans {
                if !plan.dns_managed {
                    continue;
                }
                let existing = dns_map.get(&plan.domain);
                let record = self
                    .state
                    .cf
                    .upsert_a_record(
                        &plan.domain,
                        &node.real_ip,
                        existing,
                    )
                    .await?;
                self.state
                    .db
                    .set_cloudflare_record_id(plan.id, &record.id)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn reconcile(&self) -> Result<String> {
        let _guard = self
            .lock
            .try_lock()
            .map_err(|_| anyhow!("global operation already running"))?;
        let job = Uuid::new_v4().to_string();
        reconcile::reconcile_all(&self.state, &job).await?;
        Ok(job)
    }
}

fn is_target_node_failure(err: &anyhow::Error) -> bool {
    err.to_string().starts_with("NODE_TARGET_FAILURE:")
}
