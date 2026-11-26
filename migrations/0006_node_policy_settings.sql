ALTER TABLE provider_settings ADD COLUMN probe_require_high_bandwidth INTEGER NOT NULL DEFAULT 1;
ALTER TABLE provider_settings ADD COLUMN probe_exclude_icp_required INTEGER NOT NULL DEFAULT 0;
ALTER TABLE provider_settings ADD COLUMN probe_exclude_whitelist_and_ban_overseas INTEGER NOT NULL DEFAULT 1;
ALTER TABLE provider_settings ADD COLUMN probe_max_load1 REAL NOT NULL DEFAULT 5.0;
