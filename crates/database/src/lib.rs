use anyhow::{Context, Result};
use ashan_frp_domain::{ActivityEvent, RoutingState, RoutingUpdate, TunnelPlan, TunnelPlanInput};
use chrono::{Duration, Utc};
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePoolOptions}, Row, SqlitePool};
use std::str::FromStr;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(url)
            .context("parse sqlite url")?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("connect sqlite")?;
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .context("run migrations")?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool { &self.pool }

    pub async fn seed_routing_from_env(&self, active: Option<&str>, standby: Option<&str>, days: i64, enabled: bool) -> Result<()> {
        let current = self.routing_state().await?;
        if current.active_node.is_none() && active.is_some() {
            sqlx::query("UPDATE routing_state SET active_node=?, standby_node=?, quarantine_days=?, failover_enabled=?, updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1")
                .bind(active).bind(standby).bind(days).bind(enabled as i64).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn list_tunnels(&self) -> Result<Vec<TunnelPlan>> {
        let rows = sqlx::query("SELECT id,name,local_ip,local_port,protocol,domain,dns_managed,cloudflare_record_id,enabled,created_at,updated_at FROM tunnel_plans ORDER BY name")
            .fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(row_to_tunnel).collect())
    }

    pub async fn create_tunnel(&self, input: &TunnelPlanInput) -> Result<TunnelPlan> {
        let result = sqlx::query("INSERT INTO tunnel_plans(name,local_ip,local_port,protocol,domain,dns_managed,enabled) VALUES(?,?,?,?,?,?,1)")
            .bind(input.name.trim()).bind(input.local_ip.trim()).bind(input.local_port)
            .bind(input.protocol.to_ascii_lowercase()).bind(input.domain.trim().to_ascii_lowercase())
            .bind(input.dns_managed as i64).execute(&self.pool).await?;
        self.get_tunnel(result.last_insert_rowid()).await
    }

    pub async fn update_tunnel(&self, id: i64, input: &TunnelPlanInput) -> Result<TunnelPlan> {
        sqlx::query("UPDATE tunnel_plans SET name=?,local_ip=?,local_port=?,protocol=?,domain=?,dns_managed=?,updated_at=CURRENT_TIMESTAMP WHERE id=?")
            .bind(input.name.trim()).bind(input.local_ip.trim()).bind(input.local_port)
            .bind(input.protocol.to_ascii_lowercase()).bind(input.domain.trim().to_ascii_lowercase())
            .bind(input.dns_managed as i64).bind(id).execute(&self.pool).await?;
        self.get_tunnel(id).await
    }

    pub async fn delete_tunnel(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM tunnel_plans WHERE id=?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn get_tunnel(&self, id: i64) -> Result<TunnelPlan> {
        let row = sqlx::query("SELECT id,name,local_ip,local_port,protocol,domain,dns_managed,cloudflare_record_id,enabled,created_at,updated_at FROM tunnel_plans WHERE id=?")
            .bind(id).fetch_one(&self.pool).await?;
        Ok(row_to_tunnel(row))
    }

    pub async fn set_cloudflare_record_id(&self, id: i64, record_id: &str) -> Result<()> {
        sqlx::query("UPDATE tunnel_plans SET cloudflare_record_id=?,updated_at=CURRENT_TIMESTAMP WHERE id=?")
            .bind(record_id).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn routing_state(&self) -> Result<RoutingState> {
        let row = sqlx::query("SELECT active_node,standby_node,quarantine_days,failover_enabled,state,revision,updated_at FROM routing_state WHERE singleton_id=1")
            .fetch_one(&self.pool).await?;
        Ok(RoutingState {
            active_node: row.try_get("active_node")?, standby_node: row.try_get("standby_node")?,
            quarantine_days: row.get("quarantine_days"), failover_enabled: row.get::<i64,_>("failover_enabled") != 0,
            state: row.get("state"), revision: row.get("revision"), updated_at: row.get("updated_at"),
        })
    }

    pub async fn update_routing(&self, input: &RoutingUpdate) -> Result<RoutingState> {
        sqlx::query("UPDATE routing_state SET active_node=?,standby_node=?,quarantine_days=?,failover_enabled=?,revision=revision+1,updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1")
            .bind(&input.active_node).bind(&input.standby_node).bind(input.quarantine_days)
            .bind(input.failover_enabled as i64).execute(&self.pool).await?;
        self.routing_state().await
    }

    pub async fn set_routing_state(&self, state: &str) -> Result<()> {
        sqlx::query("UPDATE routing_state SET state=?,updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1")
            .bind(state).execute(&self.pool).await?;
        Ok(())
    }

    /// Promote the FRPC-validated target before touching DNS.  From this point the
    /// control plane's runtime truth is the new node even if Cloudflare later fails.
    pub async fn promote_active_node(&self, active: &str, standby: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE routing_state SET active_node=?,standby_node=?,state='dns_switching',revision=revision+1,updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1")
            .bind(active).bind(standby).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn finalize_active_node(&self) -> Result<()> {
        sqlx::query("UPDATE routing_state SET state='idle',updated_at=CURRENT_TIMESTAMP WHERE singleton_id=1")
            .execute(&self.pool).await?;
        Ok(())
    }

    /// Preserve the most useful recovery state.  A DNS failure after target
    /// promotion is not the same as a provider/runtime failure before promotion.
    pub async fn mark_routing_failed(&self) -> Result<()> {
        let current = self.routing_state().await?;
        let state = if current.state == "dns_switching" { "degraded_dns" } else { "failed" };
        self.set_routing_state(state).await
    }

    pub async fn quarantine_node(&self, node: &str, ip: Option<&str>, reason: &str, trigger_tunnel: Option<&str>, days: i64) -> Result<()> {
        let now = Utc::now();
        let until = now + Duration::days(days.max(1));
        sqlx::query("INSERT INTO node_quarantine(node_name,node_ip,reason,trigger_tunnel,started_at,quarantine_until) VALUES(?,?,?,?,?,?) ON CONFLICT(node_name) DO UPDATE SET node_ip=excluded.node_ip,reason=excluded.reason,trigger_tunnel=excluded.trigger_tunnel,started_at=excluded.started_at,quarantine_until=excluded.quarantine_until")
            .bind(node).bind(ip).bind(reason).bind(trigger_tunnel)
            .bind(now.to_rfc3339()).bind(until.to_rfc3339()).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn quarantine_until(&self, node: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT quarantine_until FROM node_quarantine WHERE node_name=? AND quarantine_until > ?")
            .bind(node).bind(Utc::now().to_rfc3339()).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.get("quarantine_until")))
    }

    pub async fn save_config_revision(&self, node: &str, sha256: &str, config: &str) -> Result<i64> {
        let result = sqlx::query("INSERT INTO frpc_config_revisions(node_name,sha256,config_text) VALUES(?,?,?)")
            .bind(node).bind(sha256).bind(config).execute(&self.pool).await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn activity(&self, job_id: Option<&str>, kind: &str, level: &str, message: &str, tunnel: Option<&str>, node: Option<&str>, details: serde_json::Value) -> Result<()> {
        sqlx::query("INSERT INTO activity_events(job_id,kind,level,message,tunnel_name,node_name,details_json) VALUES(?,?,?,?,?,?,?)")
            .bind(job_id).bind(kind).bind(level).bind(message).bind(tunnel).bind(node)
            .bind(details.to_string()).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn recent_activity(&self, limit: i64) -> Result<Vec<ActivityEvent>> {
        let rows = sqlx::query("SELECT id,job_id,kind,level,message,tunnel_name,node_name,details_json,created_at FROM activity_events ORDER BY id DESC LIMIT ?")
            .bind(limit.clamp(1,500)).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| ActivityEvent {
            id:r.get("id"), job_id:r.try_get("job_id").ok().flatten(), kind:r.get("kind"), level:r.get("level"),
            message:r.get("message"), tunnel_name:r.try_get("tunnel_name").ok().flatten(), node_name:r.try_get("node_name").ok().flatten(),
            details:serde_json::from_str::<serde_json::Value>(&r.get::<String,_>("details_json")).unwrap_or(serde_json::json!({})), created_at:r.get("created_at")
        }).collect())
    }
}

fn row_to_tunnel(row: sqlx::sqlite::SqliteRow) -> TunnelPlan {
    TunnelPlan {
        id:row.get("id"), name:row.get("name"), local_ip:row.get("local_ip"), local_port:row.get("local_port"),
        protocol:row.get("protocol"), domain:row.get("domain"), dns_managed:row.get::<i64,_>("dns_managed") != 0,
        cloudflare_record_id:row.try_get("cloudflare_record_id").ok().flatten(), enabled:row.get::<i64,_>("enabled") != 0,
        created_at:row.get("created_at"), updated_at:row.get("updated_at")
    }
}
