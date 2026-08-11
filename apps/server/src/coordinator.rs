use crate::{reconcile, state::AppState};
use anyhow::{Context, Result, anyhow};
use ashan_frp_chmlfrp::RemoteTunnel;
use ashan_frp_cloudflare::DnsRecord;
use ashan_frp_domain::{
    FaultDomain, FrpcEvent, FrpcEventType, ProviderSettingsUpdate, ProviderSettingsView,
    RoutingPhase, TunnelPlan,
};
use ashan_frp_failover::ordered_candidates;
use ashan_frp_frpc_runtime::ReadinessError;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Error)]
enum MigrationError {
    #[error("target node runtime failure: {0}")]
    TargetNode(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum ProviderSettingsApplyError {
    #[error("global operation already running")]
    Busy,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

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

    pub async fn apply_provider_settings(
        &self,
        input: &ProviderSettingsUpdate,
    ) -> std::result::Result<ProviderSettingsView, ProviderSettingsApplyError> {
        let _guard = self
            .lock
            .try_lock()
            .map_err(|_| ProviderSettingsApplyError::Busy)?;
        let saved = self.state.db.update_provider_settings(input).await?;
        self.state
            .chml
            .reconfigure(&saved.chmlfrp_base_url, &saved.chmlfrp_token);
        self.state.cf.reconfigure(
            &saved.cloudflare_api_base,
            &saved.cloudflare_api_token,
            &saved.cloudflare_zone_id,
        );
        let view = saved.view();
        self.state
            .db
            .activity(
                None,
                "provider_settings",
                "info",
                "Provider 配置已从 WebUI 更新并立即生效",
                None,
                None,
                serde_json::json!({
                    "chmlfrp_base_url": view.chmlfrp_base_url.clone(),
                    "chmlfrp_token_configured": view.chmlfrp_token_configured,
                    "cloudflare_api_base": view.cloudflare_api_base.clone(),
                    "cloudflare_api_token_configured": view.cloudflare_api_token_configured,
                    "cloudflare_zone_id": view.cloudflare_zone_id.clone(),
                }),
            )
            .await?;
        Ok(view)
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
                            if let Err(err) = next.queued_global_failover(event).await {
                                error!(error = %err, "automatic global failover failed");
                            }
                        });
                    }
                    Ok(event) if event.event_type == FrpcEventType::NetworkAmbiguous => {
                        let next = me.clone();
                        tokio::spawn(async move {
                            if let Ok(Some(confirmed)) =
                                next.confirm_ambiguous_network_fault(event).await
                                && let Err(err) = next.queued_global_failover(confirmed).await
                            {
                                error!(error = %err, "confirmed network fault failover failed");
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

    /// Automatic FRPC faults are never discarded just because reconcile/failover
    /// currently holds the global operation lock. They wait for the lock, then the
    /// FRPC generation is checked. If another restart already replaced that process,
    /// the old event is stale and is safely ignored.
    async fn queued_global_failover(&self, event: FrpcEvent) -> Result<()> {
        let generation = event.runtime_generation;
        let _guard = self.lock.lock().await;
        if generation != self.state.frpc.generation() {
            info!(
                event_generation = generation,
                current_generation = self.state.frpc.generation(),
                "ignore stale FRPC fault after runtime generation changed"
            );
            return Ok(());
        }
        self.global_failover_locked(Some(event)).await.map(|_| ())
    }

    async fn confirm_ambiguous_network_fault(
        &self,
        mut event: FrpcEvent,
    ) -> Result<Option<FrpcEvent>> {
        if event.runtime_generation != self.state.frpc.generation() {
            return Ok(None);
        }
        let routing = self.state.db.routing_state().await?;
        let Some(active) = routing.active_node else {
            return Ok(None);
        };
        let info = match self.state.chml.node_info(&active).await {
            Ok(info) => info,
            Err(_) => return Ok(None),
        };
        if !info.state.eq_ignore_ascii_case("online") {
            event.event_type = FrpcEventType::ServerConnectionFailure;
            event.fault_domain = FaultDomain::Node;
            event.severity = "critical".into();
            event.triggers_failover = true;
            self.state
                .db
                .activity(
                    None,
                    "frpc_fault",
                    "critical",
                    "FRPC 模糊网络故障经 ChmlFrp 节点状态确认，提升为节点级故障",
                    event.proxy_name.as_deref(),
                    Some(&active),
                    serde_json::json!({"raw": event.raw, "node_state": info.state}),
                )
                .await?;
            return Ok(Some(event));
        }
        Ok(None)
    }

    pub async fn global_failover(&self, trigger: Option<FrpcEvent>) -> Result<String> {
        let _guard = self
            .lock
            .try_lock()
            .map_err(|_| anyhow!("GLOBAL_FAILOVER already running"))?;
        self.global_failover_locked(trigger).await
    }

    async fn global_failover_locked(&self, trigger: Option<FrpcEvent>) -> Result<String> {
        if let Some(event) = trigger.as_ref()
            && event.runtime_generation != self.state.frpc.generation()
        {
            return Err(anyhow!("stale FRPC failover event ignored"));
        }

        let job = Uuid::new_v4().to_string();
        *self.state.failover_job.write().await = Some(job.clone());

        let result = self.run_global_failover(&job, trigger).await;
        if let Err(err) = &result {
            let _ = self.state.db.mark_routing_failed().await;
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
        self.state
            .db
            .set_routing_phase(RoutingPhase::Failover)
            .await?;

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

            // The active node is now a confirmed node failure. Do not let frpc
            // reconnect to that known-bad node while provider/candidate preflight
            // continues. Candidate configs are started explicitly later.
            self.state
                .frpc
                .stop()
                .await
                .context("stop FRPC after confirmed active-node failure")?;
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

        let require_web = self.state.db.list_tunnels().await?.into_iter().any(|plan| {
            plan.enabled
                && matches!(
                    plan.protocol.to_ascii_lowercase().as_str(),
                    "http" | "https"
                )
        });

        let mut nodes = match self.state.chml.list_nodes().await {
            Ok(nodes) => nodes,
            Err(error) => {
                if trigger.is_none() {
                    self.state.db.set_routing_phase(RoutingPhase::Idle).await?;
                }
                return Err(error).context("list ChmlFrp failover candidates");
            }
        };
        for node in &mut nodes {
            node.quarantined_until = self.state.db.quarantine_until(&node.name).await?;
        }
        let candidates =
            ordered_candidates(&old, routing.standby_node.as_deref(), &nodes, require_web);
        if candidates.is_empty() {
            if trigger.is_none() {
                self.state.db.set_routing_phase(RoutingPhase::Idle).await?;
            }
            return Err(anyhow!("no eligible standby node"));
        }

        let mut last_error: Option<anyhow::Error> = None;
        for (idx, target) in candidates.iter().enumerate() {
            let next_standby = if trigger.is_none() {
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

            match self
                .migrate_to(
                    job,
                    &target.name,
                    next_standby,
                    trigger_tunnel,
                    trigger.is_none(),
                )
                .await
            {
                Ok(()) => {
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
                Err(MigrationError::TargetNode(message)) => {
                    // Never keep frpc running against a candidate that has just
                    // produced a confirmed node-level runtime failure.
                    let _ = self.state.frpc.stop().await;
                    self.state
                        .db
                        .quarantine_node(
                            &target.name,
                            target.real_ip.as_deref(),
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
                            "候选节点出现明确 Node 级运行故障；隔离并继续 Forward Recovery",
                            None,
                            Some(&target.name),
                            serde_json::json!({"error": message.clone()}),
                        )
                        .await?;
                    last_error = Some(anyhow!(message));
                    if trigger.is_none() && idx + 1 < candidates.len() {
                        // Manual failover starts from a healthy original node.
                        // Restore that known-good baseline before attempting another
                        // candidate so a later precommit error can never restore the
                        // just-quarantined candidate configuration.
                        self.recover_manual_original(job, &old).await?;
                        self.state
                            .db
                            .set_routing_phase(RoutingPhase::Failover)
                            .await?;
                    }
                    continue;
                }
                Err(err) => {
                    self.state
                        .db
                        .activity(
                            Some(job),
                            "failover",
                            "error",
                            "全局迁移在 Provider/配置/DNS 阶段失败；未误判候选节点故障",
                            None,
                            Some(&target.name),
                            serde_json::json!({"error": err.to_string()}),
                        )
                        .await?;

                    let current = self.state.db.routing_state().await?;
                    let precommit = current.active_node.as_deref() == Some(old.as_str())
                        && current.state == RoutingPhase::Failover;
                    if precommit {
                        if trigger.is_none() {
                            self.recover_manual_original(job, &old).await?;
                        } else {
                            // The original node was already confirmed faulty. A
                            // provider compensation may put ChmlFrp back into a
                            // consistent old-node shape, but FRPC must not resume
                            // traffic on that known-bad node.
                            let _ = self.state.frpc.stop().await;
                        }
                    }
                    return Err(err.into());
                }
            }
        }

        if trigger.is_none() {
            // A manual switch does not mean the original node is faulty. If every
            // candidate fails, restore the original global desired state instead of
            // manufacturing an outage.
            self.recover_manual_original(job, &old).await?;
        } else {
            // Every candidate produced a confirmed node-level runtime failure. Do
            // not keep FRPC running against the final failed candidate, and never
            // roll back to the original node that was already quarantined.
            let _ = self.state.frpc.stop().await;
            let _ = self
                .state
                .db
                .activity(
                    Some(job),
                    "failover",
                    "critical",
                    "所有候选节点均发生明确 Node 级故障；FRPC 已停止，等待人工恢复",
                    None,
                    None,
                    serde_json::json!({"all_candidates_failed": true}),
                )
                .await;
        }

        Err(last_error.unwrap_or_else(|| anyhow!("all failover candidates failed")))
    }

    async fn recover_manual_original(&self, job: &str, old: &str) -> Result<()> {
        match reconcile::reconcile_all(&self.state, job).await {
            Ok(()) => {
                self.state.db.set_routing_phase(RoutingPhase::Idle).await?;
                let _ = self
                    .state
                    .db
                    .activity(
                        Some(job),
                        "failover_recovery",
                        "warning",
                        "手动切换失败；已恢复原活动节点",
                        None,
                        Some(old),
                        serde_json::json!({"manual_failover_recovered": true}),
                    )
                    .await;
                Ok(())
            }
            Err(recovery_error) => {
                let _ = self.state.frpc.stop().await;
                let _ = self
                    .state
                    .db
                    .activity(
                        Some(job),
                        "failover_recovery",
                        "critical",
                        "手动切换失败且原活动节点恢复失败；FRPC 已停止",
                        None,
                        Some(old),
                        serde_json::json!({"error": recovery_error.to_string()}),
                    )
                    .await;
                Err(recovery_error)
            }
        }
    }

    async fn migrate_to(
        &self,
        job: &str,
        target: &str,
        next_standby: Option<&str>,
        trigger_tunnel: Option<&str>,
        restore_previous_runtime: bool,
    ) -> std::result::Result<(), MigrationError> {
        let plans = self
            .state
            .db
            .list_tunnels()
            .await?
            .into_iter()
            .filter(|p| p.enabled)
            .collect::<Vec<_>>();
        if plans.is_empty() {
            return Err(anyhow!("no enabled managed tunnels").into());
        }
        let dns_required = plans.iter().any(|plan| plan.dns_managed);
        if dns_required && !self.state.cf.configured() {
            return Err(anyhow!(
                "failover preflight failed: managed DNS exists but Cloudflare is not configured"
            )
            .into());
        }
        if dns_required {
            let records = self
                .state
                .cf
                .list_a_records()
                .await
                .context("failover preflight: read Cloudflare A records")?;
            ensure_unique_managed_a_records(&plans, &records)?;
        }

        // Provider preflight happens before the first mutation. API failure or a
        // missing real IP is a provider/config problem, not proof the node is bad.
        let target_info = self
            .state
            .chml
            .node_info(target)
            .await
            .context("failover preflight: read target ChmlFrp node")?;
        if !target_info.state.eq_ignore_ascii_case("online") {
            return Err(MigrationError::TargetNode(format!(
                "target node reports state {}",
                target_info.state
            )));
        }
        if dns_required && target_info.real_ip.trim().is_empty() {
            return Err(
                anyhow!("failover preflight failed: target ChmlFrp node has no realIp").into(),
            );
        }

        let remote = self.state.chml.list_tunnels().await?;
        let map: HashMap<String, RemoteTunnel> = remote
            .into_iter()
            .map(|t| (t.tunnel_name.clone(), t))
            .collect();

        // Emergency failover is not provisioning. "All tunnels together" can only
        // be guaranteed if every enabled planned tunnel already exists remotely.
        let missing = plans
            .iter()
            .filter(|plan| !map.contains_key(&plan.name))
            .map(|plan| plan.name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(anyhow!(
                "failover preflight failed: ChmlFrp tunnels missing [{}]; run global reconcile first",
                missing.join(",")
            ).into());
        }

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

        let snapshots = plans
            .iter()
            .map(|plan| {
                map.get(&plan.name)
                    .cloned()
                    .map(|remote| (plan.clone(), remote))
                    .ok_or_else(|| {
                        MigrationError::Other(anyhow!(
                            "failover preflight changed unexpectedly: tunnel {} disappeared",
                            plan.name
                        ))
                    })
            })
            .collect::<Result<Vec<_>, MigrationError>>()?;
        let mut changed = Vec::<(TunnelPlan, RemoteTunnel)>::new();

        for (plan, original) in &snapshots {
            if let Err(err) = self.sync_with_retry(original, plan, target).await {
                self.rollback_provider(job, &changed).await;
                return Err(MigrationError::Other(err.context(format!(
                    "ChmlFrp partial migration stopped at tunnel {}",
                    plan.name
                ))));
            }
            changed.push((plan.clone(), original.clone()));
        }

        if let Err(err) = self.verify_remote_target(&plans, target).await {
            self.rollback_provider(job, &changed).await;
            return Err(err.into());
        }

        let names = plans.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        let config = match self.state.chml.generated_config(target, &names).await {
            Ok(config) => config,
            Err(err) => {
                self.rollback_provider(job, &changed).await;
                return Err(MigrationError::Other(
                    err.context("generate ChmlFrp target config"),
                ));
            }
        };
        if ashan_frp_frpc_log::config_has_duplicate_proxy_names(&config) {
            self.rollback_provider(job, &changed).await;
            return Err(anyhow!("ChmlFrp generated config contains duplicate proxy names").into());
        }

        let previous_config = self.state.frpc.read_config().await?;
        let previous_revision = self.state.frpc.status().await.config_revision;
        let sha = hex::encode(Sha256::digest(config.as_bytes()));
        let revision = self
            .state
            .db
            .save_config_revision(target, &sha, &config)
            .await?;

        if let Err(err) = self.state.frpc.write_config(&config, revision).await {
            self.rollback_provider(job, &changed).await;
            return Err(MigrationError::Other(
                err.context("write target FRPC config"),
            ));
        }
        if let Err(err) = self.state.frpc.restart().await {
            self.rollback_provider(job, &changed).await;
            if restore_previous_runtime {
                self.restore_previous_frpc(previous_config.as_deref(), previous_revision)
                    .await;
            } else {
                let _ = self.state.frpc.stop().await;
            }
            return Err(MigrationError::Other(
                err.context("restart FRPC with target config"),
            ));
        }

        self.state
            .db
            .activity(
                Some(job),
                "failover",
                "info",
                "FRPC：已加载目标节点完整配置，等待运行验证",
                None,
                Some(target),
                serde_json::json!({"revision": revision, "generation": self.state.frpc.generation()}),
            )
            .await?;

        match self
            .state
            .frpc
            .wait_ready(&names, Duration::from_secs(20))
            .await
        {
            Ok(()) => {}
            Err(ReadinessError::Node(err)) => {
                return Err(MigrationError::TargetNode(err));
            }
            Err(ReadinessError::Timeout { connected, missing }) => {
                // A timeout alone does not prove the standby is bad. Promote to a
                // node failure only when ChmlFrp independently reports it offline.
                match self.state.chml.node_info(target).await {
                    Ok(info) if !info.state.eq_ignore_ascii_case("online") => {
                        return Err(MigrationError::TargetNode(format!(
                            "readiness timeout and node state={}",
                            info.state
                        )));
                    }
                    _ => {
                        self.rollback_provider(job, &changed).await;
                        if restore_previous_runtime {
                            self.restore_previous_frpc(
                                previous_config.as_deref(),
                                previous_revision,
                            )
                            .await;
                        } else {
                            let _ = self.state.frpc.stop().await;
                        }
                        return Err(anyhow!(
                            "FRPC readiness timeout without confirmed node failure; connected={connected}, missing={missing}"
                        ).into());
                    }
                }
            }
            Err(ReadinessError::NonNode(err)) => {
                self.rollback_provider(job, &changed).await;
                if restore_previous_runtime {
                    self.restore_previous_frpc(previous_config.as_deref(), previous_revision)
                        .await;
                } else {
                    let _ = self.state.frpc.stop().await;
                }
                return Err(anyhow!("FRPC non-node startup failure: {err}").into());
            }
        }

        // Runtime truth changes here, BEFORE DNS. If Cloudflare fails afterwards,
        // DB/FRPC/ChmlFrp still agree on the new active node and state becomes
        // degraded_dns so a later reconcile can finish the A-record convergence.
        self.state
            .db
            .promote_active_node(target, next_standby)
            .await?;

        if dns_required {
            self.state
                .db
                .activity(
                    Some(job),
                    "failover",
                    "info",
                    "Cloudflare：FRPC 新链路验证后统一更新全部受管 A 记录",
                    trigger_tunnel,
                    Some(target),
                    serde_json::json!({"ip": target_info.real_ip.clone()}),
                )
                .await?;
            let records = self.state.cf.list_a_records().await?;
            ensure_unique_managed_a_records(&plans, &records)?;
            let dns_map: HashMap<String, _> = records
                .into_iter()
                .map(|record| (record.name.clone(), record))
                .collect();
            for plan in &plans {
                if !plan.dns_managed {
                    continue;
                }
                let existing = dns_map.get(&plan.domain);
                let record = self
                    .state
                    .cf
                    .upsert_a_record(&plan.domain, &target_info.real_ip, existing)
                    .await
                    .with_context(|| format!("update Cloudflare A record {}", plan.domain))?;
                self.state
                    .db
                    .set_cloudflare_record_id(plan.id, &record.id)
                    .await?;
            }
        }

        self.state.db.finalize_active_node().await?;
        Ok(())
    }

    async fn sync_with_retry(
        &self,
        remote: &RemoteTunnel,
        plan: &TunnelPlan,
        target: &str,
    ) -> Result<()> {
        let mut last = None;
        for attempt in 1..=3 {
            match self.state.chml.sync_tunnel(remote, plan, target).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last = Some(err);
                    if attempt < 3 {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
        Err(last.unwrap_or_else(|| anyhow!("unknown ChmlFrp sync failure")))
    }

    async fn verify_remote_target(&self, plans: &[TunnelPlan], target: &str) -> Result<()> {
        let remote = self.state.chml.list_tunnels().await?;
        let map: HashMap<String, RemoteTunnel> = remote
            .into_iter()
            .map(|t| (t.tunnel_name.clone(), t))
            .collect();
        let mut problems = Vec::new();
        for plan in plans {
            match map.get(&plan.name) {
                Some(remote) if remote.matches_plan_on_node(plan, target) => {}
                Some(remote) => problems.push(format!(
                    "{}: node={} local={}:{} protocol={} domain={}",
                    plan.name,
                    remote.node,
                    remote.local_ip,
                    remote.local_port,
                    remote.port_type,
                    remote.effective_domain()
                )),
                None => problems.push(format!("{}: missing after migration", plan.name)),
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(anyhow!(
                "ChmlFrp target verification failed: {}",
                problems.join("; ")
            ))
        }
    }

    async fn rollback_provider(&self, job: &str, changed: &[(TunnelPlan, RemoteTunnel)]) {
        if changed.is_empty() {
            return;
        }
        let mut failures = Vec::new();
        for (plan, original) in changed.iter().rev() {
            if let Err(err) = self.sync_with_retry(original, plan, &original.node).await {
                failures.push(format!("{}: {}", plan.name, err));
            }
        }
        let level = if failures.is_empty() {
            "warning"
        } else {
            "error"
        };
        let message = if failures.is_empty() {
            "ChmlFrp 预提交阶段失败，已补偿恢复所有已修改隧道"
        } else {
            "ChmlFrp 补偿恢复不完整，需要人工检查"
        };
        let _ = self
            .state
            .db
            .activity(
                Some(job),
                "failover_rollback",
                level,
                message,
                None,
                None,
                serde_json::json!({"failures": failures}),
            )
            .await;
    }

    async fn restore_previous_frpc(&self, config: Option<&str>, revision: Option<i64>) {
        let Some(config) = config else {
            return;
        };
        if self
            .state
            .frpc
            .restore_config(config, revision)
            .await
            .is_ok()
        {
            let _ = self.state.frpc.restart().await;
        }
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
            "Cloudflare managed A-record conflict: {}; failover requires exactly zero or one A record per managed domain",
            conflicts.join(", ")
        ))
    }
}
