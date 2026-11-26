-- Migration 0005: ChmlFrp multi-method authentication (SSO refresh token + Qzhua credentials + TOTP 2FA)
ALTER TABLE provider_settings ADD COLUMN chmlfrp_refresh_token TEXT NOT NULL DEFAULT '';
ALTER TABLE provider_settings ADD COLUMN chmlfrp_token_expires_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE provider_settings ADD COLUMN chmlfrp_username TEXT NOT NULL DEFAULT '';
ALTER TABLE provider_settings ADD COLUMN chmlfrp_password TEXT NOT NULL DEFAULT '';
ALTER TABLE provider_settings ADD COLUMN chmlfrp_totp_secret TEXT NOT NULL DEFAULT '';
