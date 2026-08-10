CREATE TABLE tunnel_plans_v2 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    local_ip TEXT NOT NULL,
    local_port INTEGER NOT NULL CHECK(local_port BETWEEN 1 AND 65535),
    protocol TEXT NOT NULL DEFAULT 'http' CHECK(protocol IN ('http', 'https', 'tcp', 'udp')),
    domain TEXT NOT NULL DEFAULT '',
    dns_managed INTEGER NOT NULL DEFAULT 1 CHECK(dns_managed IN (0, 1)),
    cloudflare_record_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK(domain <> '' OR (protocol IN ('tcp', 'udp') AND dns_managed = 0))
);

INSERT INTO tunnel_plans_v2 (
    id,
    name,
    local_ip,
    local_port,
    protocol,
    domain,
    dns_managed,
    cloudflare_record_id,
    enabled,
    created_at,
    updated_at
)
SELECT
    id,
    name,
    local_ip,
    local_port,
    lower(protocol),
    lower(domain),
    dns_managed,
    cloudflare_record_id,
    enabled,
    created_at,
    updated_at
FROM tunnel_plans;

DROP TABLE tunnel_plans;
ALTER TABLE tunnel_plans_v2 RENAME TO tunnel_plans;

CREATE UNIQUE INDEX idx_tunnel_plans_managed_domain
    ON tunnel_plans(domain)
    WHERE dns_managed = 1 AND domain <> '';
