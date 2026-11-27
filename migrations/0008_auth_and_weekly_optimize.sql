ALTER TABLE provider_settings ADD COLUMN control_api_key TEXT NOT NULL DEFAULT '';
ALTER TABLE provider_settings ADD COLUMN weekly_optimize_enabled INTEGER NOT NULL DEFAULT 1;
ALTER TABLE provider_settings ADD COLUMN weekly_optimize_weekday INTEGER NOT NULL DEFAULT 0;
ALTER TABLE provider_settings ADD COLUMN weekly_optimize_hour INTEGER NOT NULL DEFAULT 3;
ALTER TABLE provider_settings ADD COLUMN weekly_optimize_min_improvement_ms REAL NOT NULL DEFAULT 15.0;
ALTER TABLE provider_settings ADD COLUMN last_weekly_optimize_at TEXT NOT NULL DEFAULT '';
