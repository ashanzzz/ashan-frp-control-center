CREATE TABLE IF NOT EXISTS provider_settings (
    singleton_id INTEGER PRIMARY KEY CHECK(singleton_id = 1),
    chmlfrp_base_url TEXT NOT NULL DEFAULT 'https://cf-v2.uapis.cn',
    chmlfrp_token TEXT NOT NULL DEFAULT '',
    cloudflare_api_base TEXT NOT NULL DEFAULT 'https://api.cloudflare.com/client/v4',
    cloudflare_api_token TEXT NOT NULL DEFAULT '',
    cloudflare_zone_id TEXT NOT NULL DEFAULT '',
    env_bootstrap_complete INTEGER NOT NULL DEFAULT 0 CHECK(env_bootstrap_complete IN (0,1)),
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT OR IGNORE INTO provider_settings(singleton_id) VALUES (1);
