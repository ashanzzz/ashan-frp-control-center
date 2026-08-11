use anyhow::{Context, Result};
use ashan_frp_domain::{
    ActivityEvent, ProviderSettingsUpdate, ProviderSettingsView, RoutingPhase, RoutingState,
    RoutingUpdate, TunnelPlan, TunnelPlanInput,
};
use chrono::{Duration, Utc};
use serde_json::Value;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{str::FromStr, time::Duration as StdDuration};

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct ProviderSettings {
    pub chmlfrp_base_url: String,
    pub chmlfrp_token: String,
    pub cloudflare_api_base: String,
    pub cloudflare_api_token: String,
    pub cloudflare_zone_id: String,
    pub updated_at: String,
}

impl ProviderSettings {
    pub fn view(&self) -> ProviderSettingsView {
        ProviderSettingsView {
            chmlfrp_base_url: self.chmlfrp_base_url.clone(),
            chmlfrp_token_configured: !self.chmlfrp_token.trim().is_empty(),
            cloudflare_api_base: self.cloudflare_api_base.clone(),
            cloudflare_api_token_configured: !self.cloudflare_api_token.trim().is_empty(),
            cloudflare_zone_id: self.cloudflare_zone_id.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(url)
            .context("parse sqlite url")?
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(StdDuration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("connect sqlite")?;

        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await
            .context("enable sqlite WAL mode")?;
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .context("run migrations")?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn provider_settings(&self) -> Result<ProviderSettings> {
        let row = sqlx::query(
            "SELECT chmlfrp_base_url,chmlfrp_token,cloudflare_api_base,cloudflare_api_token,\
             cloudflare_zone_id,updated_at FROM provider_settings WHERE singleton_id=1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(ProviderSettings {
            chmlfrp_base_url: row.get("chmlfrp_base_url"),
            chmlfrp_token: row.get("chmlfrp_token"),
            cloudflare_api_base: row.get("cloudflare_api_base"),
            cloudflare_api_token: row.get("cloudflare_api_token"),
            cloudflare_zone_id: row.get("cloudflare_zone_id"),
            updated_at: row.get("updated_at"),
        })
    }

    /// Environment variables are a one-shot compatibility bootstrap only.
    /// After the first v0.3+ startup, WebUI/SQLite is the source of truth, so
    /// explicitly clearing a token cannot be undone by a stale Docker variable.
    #[allow(clippy::too_many_arguments)]
    pub async fn seed_provider_settings_from_env(
        &self,
        chmlfrp_base_url: &str,
        chmlfrp_token: &str,
        cloudflare_api_base: &str,
        cloudflare_api_token: &str,
        cloudflare_zone_id: &str,
    ) -> Result<()> {
        let bootstrap_complete: i64 = sqlx::query_scalar(
            "SELECT env_bootstrap_complete FROM provider_settings WHERE singleton_id=1",
        )
        .fetch_one(&self.pool)
        .await?;
        if bootstrap_complete != 0 {
            return Ok(());
        }

        sqlx::query(
            "UPDATE provider_settings SET chmlfrp_base_url=?,chmlfrp_token=?,\
             cloudflare_api_base=?,cloudflare_api_token=?,cloudflare_zone_id=?,\
             env_bootstrap_complete=1,updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1",
        )
        .bind(chmlfrp_base_url.trim().trim_end_matches('/'))
        .bind(chmlfrp_token.trim())
        .bind(cloudflare_api_base.trim().trim_end_matches('/'))
        .bind(cloudflare_api_token.trim())
        .bind(cloudflare_zone_id.trim())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_provider_settings(
        &self,
        input: &ProviderSettingsUpdate,
    ) -> Result<ProviderSettings> {
        let current = self.provider_settings().await?;
        let chmlfrp_token = if input.clear_chmlfrp_token {
            String::new()
        } else {
            input
                .chmlfrp_token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .unwrap_or(current.chmlfrp_token)
        };
        let cloudflare_api_token = if input.clear_cloudflare_api_token {
            String::new()
        } else {
            input
                .cloudflare_api_token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .unwrap_or(current.cloudflare_api_token)
        };

        sqlx::query(
            "UPDATE provider_settings SET chmlfrp_base_url=?,chmlfrp_token=?,\
             cloudflare_api_base=?,cloudflare_api_token=?,cloudflare_zone_id=?,\
             env_bootstrap_complete=1,updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1",
        )
        .bind(input.chmlfrp_base_url.trim().trim_end_matches('/'))
        .bind(chmlfrp_token)
        .bind(input.cloudflare_api_base.trim().trim_end_matches('/'))
        .bind(cloudflare_api_token)
        .bind(input.cloudflare_zone_id.trim())
        .execute(&self.pool)
        .await?;
        self.provider_settings().await
    }

    pub async fn seed_routing_from_env(
        &self,
        active: Option<&str>,
        standby: Option<&str>,
        days: i64,
        enabled: bool,
    ) -> Result<()> {
        let current = self.routing_state().await?;
        if current.active_node.is_none() && active.is_some() {
            sqlx::query(
                "UPDATE routing_state SET active_node=?, standby_node=?, quarantine_days=?, \
                 failover_enabled=?, updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1",
            )
            .bind(active)
            .bind(standby)
            .bind(days)
            .bind(enabled as i64)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn list_tunnels(&self) -> Result<Vec<TunnelPlan>> {
        let rows = sqlx::query(
            "SELECT id,name,local_ip,local_port,protocol,domain,dns_managed,\
             cloudflare_record_id,enabled,created_at,updated_at \
             FROM tunnel_plans ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_tunnel).collect())
    }

    pub async fn create_tunnel(&self, input: &TunnelPlanInput) -> Result<TunnelPlan> {
        let result = sqlx::query(
            "INSERT INTO tunnel_plans(name,local_ip,local_port,protocol,domain,dns_managed,enabled) \
             VALUES(?,?,?,?,?,?,1)",
        )
        .bind(input.name.trim())
        .bind(input.local_ip.trim())
        .bind(input.local_port)
        .bind(input.protocol.to_ascii_lowercase())
        .bind(input.domain.trim().to_ascii_lowercase())
        .bind(input.dns_managed as i64)
        .execute(&self.pool)
        .await?;
        self.get_tunnel(result.last_insert_rowid()).await
    }

    pub async fn update_tunnel(&self, id: i64, input: &TunnelPlanInput) -> Result<TunnelPlan> {
        sqlx::query(
            "UPDATE tunnel_plans SET name=?,local_ip=?,local_port=?,protocol=?,domain=?,\
             dns_managed=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
        )
        .bind(input.name.trim())
        .bind(input.local_ip.trim())
        .bind(input.local_port)
        .bind(input.protocol.to_ascii_lowercase())
        .bind(input.domain.trim().to_ascii_lowercase())
        .bind(input.dns_managed as i64)
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.get_tunnel(id).await
    }

    pub async fn delete_tunnel(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM tunnel_plans WHERE id=?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_tunnel(&self, id: i64) -> Result<TunnelPlan> {
        let row = sqlx::query(
            "SELECT id,name,local_ip,local_port,protocol,domain,dns_managed,\
             cloudflare_record_id,enabled,created_at,updated_at \
             FROM tunnel_plans WHERE id=?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row_to_tunnel(row))
    }

    pub async fn set_cloudflare_record_id(&self, id: i64, record_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE tunnel_plans SET cloudflare_record_id=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
        )
        .bind(record_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn routing_state(&self) -> Result<RoutingState> {
        let row = sqlx::query(
            "SELECT active_node,standby_node,quarantine_days,failover_enabled,state,revision,updated_at \
             FROM routing_state WHERE singleton_id=1",
        )
        .fetch_one(&self.pool)
        .await?;
        let raw_state: String = row.get("state");
        let state = RoutingPhase::from_str(&raw_state).map_err(anyhow::Error::msg)?;

        Ok(RoutingState {
            active_node: row.try_get("active_node")?,
            standby_node: row.try_get("standby_node")?,
            quarantine_days: row.get("quarantine_days"),
            failover_enabled: row.get::<i64, _>("failover_enabled") != 0,
            state,
            revision: row.get("revision"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn update_routing(&self, input: &RoutingUpdate) -> Result<RoutingState> {
        sqlx::query(
            "UPDATE routing_state SET active_node=?,standby_node=?,quarantine_days=?,\
             failover_enabled=?,revision=revision+1,updated_at=CURRENT_TIMESTAMP \
             WHERE singleton_id=1",
        )
        .bind(&input.active_node)
        .bind(&input.standby_node)
        .bind(input.quarantine_days)
        .bind(input.failover_enabled as i64)
        .execute(&self.pool)
        .await?;
        self.routing_state().await
    }

    pub async fn set_routing_phase(&self, phase: RoutingPhase) -> Result<()> {
        sqlx::query(
            "UPDATE routing_state SET state=?,updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1",
        )
        .bind(phase.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn promote_active_node(&self, active: &str, standby: Option<&str>) -> Result<()> {
        sqlx::query(
            "UPDATE routing_state SET active_node=?,standby_node=?,state='dns_switching',\
             revision=revision+1,updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1",
        )
        .bind(active)
        .bind(standby)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn finalize_active_node(&self) -> Result<()> {
        self.set_routing_phase(RoutingPhase::Idle).await
    }

    pub async fn mark_routing_failed(&self) -> Result<()> {
        let current = self.routing_state().await?;
        let phase = match current.state {
            RoutingPhase::Idle => RoutingPhase::Idle,
            RoutingPhase::DnsSwitching => RoutingPhase::DegradedDns,
            _ => RoutingPhase::Failed,
        };
        self.set_routing_phase(phase).await
    }

    pub async fn quarantine_node(
        &self,
        node: &str,
        ip: Option<&str>,
        reason: &str,
        trigger_tunnel: Option<&str>,
        days: i64,
    ) -> Result<()> {
        let now = Utc::now();
        let until = now + Duration::days(days.max(1));
        sqlx::query(
            "INSERT INTO node_quarantine(node_name,node_ip,reason,trigger_tunnel,started_at,quarantine_until) \
             VALUES(?,?,?,?,?,?) ON CONFLICT(node_name) DO UPDATE SET \
             node_ip=excluded.node_ip,reason=excluded.reason,trigger_tunnel=excluded.trigger_tunnel,\
             started_at=excluded.started_at,quarantine_until=excluded.quarantine_until",
        )
        .bind(node)
        .bind(ip)
        .bind(reason)
        .bind(trigger_tunnel)
        .bind(now.to_rfc3339())
        .bind(until.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn clear_quarantine(&self, node: &str) -> Result<()> {
        sqlx::query("DELETE FROM node_quarantine WHERE node_name=?")
            .bind(node)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn quarantine_until(&self, node: &str) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT quarantine_until FROM node_quarantine WHERE node_name=? AND quarantine_until > ?",
        )
        .bind(node)
        .bind(Utc::now().to_rfc3339())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.get("quarantine_until")))
    }

    pub async fn save_config_revision(
        &self,
        node: &str,
        sha256: &str,
        config: &str,
    ) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO frpc_config_revisions(node_name,sha256,config_text) VALUES(?,?,?)",
        )
        .bind(node)
        .bind(sha256)
        .bind(config)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Persist a structured activity event. The arguments intentionally mirror the
    /// seven event columns in SQLite, which keeps call sites explicit and audit-friendly.
    #[allow(clippy::too_many_arguments)]
    pub async fn activity(
        &self,
        job_id: Option<&str>,
        kind: &str,
        level: &str,
        message: &str,
        tunnel_name: Option<&str>,
        node_name: Option<&str>,
        details: Value,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO activity_events(job_id,kind,level,message,tunnel_name,node_name,details_json) \
             VALUES(?,?,?,?,?,?,?)",
        )
        .bind(job_id)
        .bind(kind)
        .bind(level)
        .bind(message)
        .bind(tunnel_name)
        .bind(node_name)
        .bind(details.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn recent_activity(&self, limit: i64) -> Result<Vec<ActivityEvent>> {
        let rows = sqlx::query(
            "SELECT id,job_id,kind,level,message,tunnel_name,node_name,details_json,created_at \
             FROM activity_events ORDER BY id DESC LIMIT ?",
        )
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ActivityEvent {
                id: row.get("id"),
                job_id: row.try_get("job_id").ok().flatten(),
                kind: row.get("kind"),
                level: row.get("level"),
                message: row.get("message"),
                tunnel_name: row.try_get("tunnel_name").ok().flatten(),
                node_name: row.try_get("node_name").ok().flatten(),
                details: serde_json::from_str::<Value>(&row.get::<String, _>("details_json"))
                    .unwrap_or_else(|_| serde_json::json!({})),
                created_at: row.get("created_at"),
            })
            .collect())
    }
}

fn row_to_tunnel(row: sqlx::sqlite::SqliteRow) -> TunnelPlan {
    TunnelPlan {
        id: row.get("id"),
        name: row.get("name"),
        local_ip: row.get("local_ip"),
        local_port: row.get("local_port"),
        protocol: row.get("protocol"),
        domain: row.get("domain"),
        dns_managed: row.get::<i64, _>("dns_managed") != 0,
        cloudflare_record_id: row.try_get("cloudflare_record_id").ok().flatten(),
        enabled: row.get::<i64, _>("enabled") != 0,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}
