PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tunnel_plans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    local_ip TEXT NOT NULL,
    local_port INTEGER NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'http',
    domain TEXT NOT NULL UNIQUE,
    dns_managed INTEGER NOT NULL DEFAULT 1,
    cloudflare_record_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS routing_state (
    singleton_id INTEGER PRIMARY KEY CHECK(singleton_id = 1),
    active_node TEXT,
    standby_node TEXT,
    quarantine_days INTEGER NOT NULL DEFAULT 30,
    failover_enabled INTEGER NOT NULL DEFAULT 1,
    state TEXT NOT NULL DEFAULT 'idle',
    revision INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT OR IGNORE INTO routing_state(singleton_id) VALUES (1);

CREATE TABLE IF NOT EXISTS node_quarantine (
    node_name TEXT PRIMARY KEY,
    node_ip TEXT,
    reason TEXT NOT NULL,
    trigger_tunnel TEXT,
    started_at TEXT NOT NULL,
    quarantine_until TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS frpc_config_revisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_name TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    config_text TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS activity_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT,
    kind TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    tunnel_name TEXT,
    node_name TEXT,
    details_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_activity_events_created_at ON activity_events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_activity_events_job_id ON activity_events(job_id);
