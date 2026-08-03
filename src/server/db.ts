import { DatabaseSync } from 'node:sqlite'
import { config } from './config.ts'
import { nowIso } from './util.ts'

export const db = new DatabaseSync(config.databasePath)
db.exec('PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;')

db.exec(`
CREATE TABLE IF NOT EXISTS admin_users(id TEXT PRIMARY KEY, username TEXT UNIQUE NOT NULL, display_name TEXT NOT NULL, password_hash TEXT NOT NULL, password_salt TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_login_at TEXT);
CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES admin_users(id) ON DELETE CASCADE, token_hash TEXT UNIQUE NOT NULL, csrf_token TEXT NOT NULL, created_at TEXT NOT NULL, expires_at TEXT NOT NULL, last_seen_at TEXT NOT NULL, user_agent TEXT, ip_address TEXT);
CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token_hash); CREATE INDEX IF NOT EXISTS idx_sessions_expiry ON sessions(expires_at);
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by TEXT);
CREATE TABLE IF NOT EXISTS credentials(id TEXT PRIMARY KEY, provider TEXT NOT NULL, name TEXT NOT NULL, secret_ciphertext TEXT NOT NULL, metadata_json TEXT NOT NULL DEFAULT '{}', status TEXT NOT NULL DEFAULT 'configured', revision INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_validated_at TEXT, last_error TEXT, UNIQUE(provider,name));
CREATE TABLE IF NOT EXISTS cache_entries(key TEXT PRIMARY KEY, provider TEXT NOT NULL, value_json TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'fresh', record_count INTEGER NOT NULL DEFAULT 0, payload_hash TEXT, updated_at TEXT NOT NULL, expires_at TEXT, last_error TEXT);
CREATE TABLE IF NOT EXISTS nodes(id TEXT PRIMARY KEY, provider TEXT NOT NULL DEFAULT 'chmlfrp', external_id TEXT, name TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL, region TEXT, endpoint TEXT, real_ip TEXT, server_port INTEGER, online INTEGER NOT NULL DEFAULT 0, web_supported INTEGER NOT NULL DEFAULT 0, latency_ms REAL, packet_loss REAL, score REAL NOT NULL DEFAULT 0, failure_count INTEGER NOT NULL DEFAULT 0, success_count INTEGER NOT NULL DEFAULT 0, ban_until TEXT, last_seen_at TEXT, last_tested_at TEXT, metadata_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_nodes_score ON nodes(online,score DESC); CREATE INDEX IF NOT EXISTS idx_nodes_ban ON nodes(ban_until);
CREATE TABLE IF NOT EXISTS node_metrics(id TEXT PRIMARY KEY, node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE, online INTEGER NOT NULL, latency_ms REAL, packet_loss REAL, score REAL, reason TEXT, measured_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_node_metrics ON node_metrics(node_id,measured_at DESC);
CREATE TABLE IF NOT EXISTS desired_tunnels(id TEXT PRIMARY KEY, name TEXT UNIQUE NOT NULL, protocol TEXT NOT NULL, local_ip TEXT NOT NULL, local_port INTEGER NOT NULL, remote_port INTEGER, domain TEXT, encryption INTEGER NOT NULL DEFAULT 0, compression INTEGER NOT NULL DEFAULT 1, extra_params TEXT NOT NULL DEFAULT '', enabled INTEGER NOT NULL DEFAULT 1, managed INTEGER NOT NULL DEFAULT 1, health_url TEXT, metadata_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS observed_tunnels(id TEXT PRIMARY KEY, external_id TEXT, name TEXT NOT NULL, node_name TEXT, protocol TEXT, local_ip TEXT, local_port INTEGER, remote_port INTEGER, domain TEXT, remote_ip TEXT, managed INTEGER NOT NULL DEFAULT 0, raw_json TEXT NOT NULL DEFAULT '{}', observed_at TEXT NOT NULL, UNIQUE(name,node_name));
CREATE TABLE IF NOT EXISTS desired_dns(id TEXT PRIMARY KEY, name TEXT NOT NULL, type TEXT NOT NULL, content TEXT NOT NULL, proxied INTEGER NOT NULL DEFAULT 0, ttl INTEGER NOT NULL DEFAULT 1, enabled INTEGER NOT NULL DEFAULT 1, managed INTEGER NOT NULL DEFAULT 1, source_tunnel_id TEXT REFERENCES desired_tunnels(id) ON DELETE SET NULL, comment TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, UNIQUE(name,type));
CREATE TABLE IF NOT EXISTS observed_dns(id TEXT PRIMARY KEY, external_id TEXT NOT NULL UNIQUE, name TEXT NOT NULL, type TEXT NOT NULL, content TEXT NOT NULL, proxied INTEGER NOT NULL DEFAULT 0, ttl INTEGER, managed INTEGER NOT NULL DEFAULT 0, raw_json TEXT NOT NULL DEFAULT '{}', observed_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sync_state(id TEXT PRIMARY KEY, subject_type TEXT NOT NULL, subject_id TEXT NOT NULL, desired_hash TEXT, observed_hash TEXT, status TEXT NOT NULL, conflict_reason TEXT, last_job_id TEXT, last_success_at TEXT, last_attempt_at TEXT, last_error TEXT, metadata_json TEXT NOT NULL DEFAULT '{}', updated_at TEXT NOT NULL, UNIQUE(subject_type,subject_id));
CREATE TABLE IF NOT EXISTS frpc_configs(id TEXT PRIMARY KEY, node_name TEXT NOT NULL, content TEXT NOT NULL, content_hash TEXT NOT NULL, source TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 0, installed_at TEXT, created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS health_snapshots(id TEXT PRIMARY KEY, overall_status TEXT NOT NULL, layers_json TEXT NOT NULL, proxy_total INTEGER NOT NULL DEFAULT 0, proxy_healthy INTEGER NOT NULL DEFAULT 0, node_name TEXT, created_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_health_created ON health_snapshots(created_at DESC);
CREATE TABLE IF NOT EXISTS switch_plans(id TEXT PRIMARY KEY, source_node TEXT, target_node TEXT NOT NULL, status TEXT NOT NULL, risk TEXT NOT NULL, reason TEXT NOT NULL, plan_json TEXT NOT NULL, rollback_json TEXT NOT NULL DEFAULT '{}', created_by TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, executed_at TEXT, completed_at TEXT, error TEXT);
CREATE TABLE IF NOT EXISTS oauth_challenges(id TEXT PRIMARY KEY, provider TEXT NOT NULL, kind TEXT NOT NULL, status TEXT NOT NULL, session_tag TEXT, payload_ciphertext TEXT, code_ciphertext TEXT, expires_at TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, type TEXT NOT NULL, target_type TEXT, target_id TEXT, idempotency_key TEXT UNIQUE, priority INTEGER NOT NULL DEFAULT 50, status TEXT NOT NULL, run_after TEXT NOT NULL, locked_at TEXT, locked_by TEXT, attempt_count INTEGER NOT NULL DEFAULT 0, max_attempts INTEGER NOT NULL DEFAULT 3, payload_json TEXT NOT NULL DEFAULT '{}', result_json TEXT, error_code TEXT, error_message TEXT, requested_by TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, started_at TEXT, completed_at TEXT);
CREATE INDEX IF NOT EXISTS idx_jobs_queue ON jobs(status,run_after,priority DESC,created_at);
CREATE TABLE IF NOT EXISTS job_events(id TEXT PRIMARY KEY, job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE, sequence_no INTEGER NOT NULL, event_type TEXT NOT NULL, level TEXT NOT NULL, message TEXT NOT NULL, payload_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, UNIQUE(job_id,sequence_no));
CREATE TABLE IF NOT EXISTS audit_logs(id TEXT PRIMARY KEY, actor_user_id TEXT, action TEXT NOT NULL, target_type TEXT, target_id TEXT, outcome TEXT NOT NULL, request_id TEXT, details_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_logs(created_at DESC);
CREATE TABLE IF NOT EXISTS locks(name TEXT PRIMARY KEY, owner TEXT NOT NULL, expires_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS scheduler_state(key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at TEXT NOT NULL);
`)

const defaults: Record<string, unknown> = {
  'app.version': '1.1.0',
  'unraid.graphql_path': '/graphql',
  'unraid.timeout_ms': 15000,
  'chmlfrp.base_url': 'https://cf-v2.uapis.cn',
  'chmlfrp.oauth.token_url': 'https://account-api.qzhua.net/oauth2/token',
  'chmlfrp.oauth.device_authorization_url': '',
  'chmlfrp.oauth.scope': '',
  'chmlfrp.managed_prefix': '[ashan-frp]',
  'chmlfrp.timeout_ms': 20000,
  'chmlfrp.tunnel_limit': 16,
  'cloudflare.api_base': 'https://api.cloudflare.com/client/v4',
  'cloudflare.zone_id': '',
  'cloudflare.default_proxied': false,
  'cloudflare.target_template': '{node}.ip.chmlfrp.cn',
  'runtime.current_node': '',
  'runtime.mode': 'embedded',
  'runtime.binary_path': process.env.FRPC_BINARY_PATH || '/usr/local/bin/frpc',
  'runtime.config_path': process.env.FRPC_CONFIG_PATH || '/data/frpc/conf/frpc.toml',
  'runtime.backup_dir': process.env.FRPC_BACKUP_DIR || '/data/backups/frpc',
  'runtime.log_path': process.env.FRPC_LOG_PATH || '/data/frpc/logs/frpc.log',
  'runtime.autostart': true,
  'runtime.auto_restart': true,
  'automation.enabled': false,
  'automation.health_interval_seconds': 60,
  'automation.failure_threshold': 3,
  'automation.recovery_threshold': 2,
  'automation.cooldown_minutes': 30,
  'automation.ban_minutes': 360,
  'automation.max_candidates': 3,
  'automation.max_latency_ms': 250,
  'automation.max_packet_loss': 30,
  'automation.require_approval_for_high_risk': true,
  'health.http_timeout_ms': 8000,
  'health.tcp_timeout_ms': 3000,
  'switch.allow_dns_failure': false,
  'email.webhook_enabled': false
}
const insert = db.prepare('INSERT OR IGNORE INTO settings(key,value_json,updated_at) VALUES(?,?,?)')
for (const [key,value] of Object.entries(defaults)) insert.run(key, JSON.stringify(value), nowIso())

export function one(sql: string, ...params: any[]): any { return db.prepare(sql).get(...params) as any }
export function all(sql: string, ...params: any[]): any[] { return db.prepare(sql).all(...params) as any[] }
export function run(sql: string, ...params: any[]): any { return db.prepare(sql).run(...params) }
export function transaction<T>(fn: () => T): T { db.exec('BEGIN IMMEDIATE'); try { const value=fn(); db.exec('COMMIT'); return value } catch(e){ db.exec('ROLLBACK'); throw e } }
