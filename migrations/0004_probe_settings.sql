-- Migration 0004: node latency probe settings
-- SQLite 3.37.0+ supports ALTER TABLE ADD COLUMN IF NOT EXISTS.
-- sqlx-migrate uses incremental numbered files, so this will only run once.
ALTER TABLE provider_settings ADD COLUMN probe_port INTEGER NOT NULL DEFAULT 7000;
ALTER TABLE provider_settings ADD COLUMN probe_timeout_ms INTEGER NOT NULL DEFAULT 2000;
ALTER TABLE provider_settings ADD COLUMN probe_latency_tolerance_ms INTEGER NOT NULL DEFAULT 50;
