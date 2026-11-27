# HTTP API

All JSON success responses use `{ "data": ... }`. Errors use `{ "code": "...", "message": "..." }`.

## Authentication

Control Center enforces API Key authentication across WebUI and `/api/v1/*` (except `/api/v1/health` and `/api/v1/auth/verify`).
Supply the key via `Authorization: Bearer <API_KEY>` or `X-API-Key: <API_KEY>` (SSE `/api/v1/events` uses `?api_key=<API_KEY>`).

- `GET /api/v1/health` liveness and version (public)
- `POST /api/v1/auth/verify` verify Control Center API Key (public)
- `PUT /api/v1/auth/key` rotate or set Control Center API Key (`{ api_key? }`)
- `GET /api/v1/capabilities` machine-readable AI self-discovery spec, live state, invariants, and endpoint index
- `GET /llms.txt` compact Markdown operation guide for AI agents
- `GET /api/v1/ready` SQLite readiness
- `GET /api/v1/dashboard` aggregated control-plane snapshot (includes `require_web`, `active_node`, `frpc.last_connected_at`, `frpc.disconnected_since`)
- `GET|POST /api/v1/tunnels` list/create plans
- `POST /api/v1/tunnels/import` import a single ChmlFrp tunnel as a managed plan
- `POST /api/v1/tunnels/import-all` batch import unmanaged ChmlFrp tunnels
- `POST /api/v1/tunnels/{id}/unmanage` remove local plan without altering remote resources
- `PUT|DELETE /api/v1/tunnels/{id}` update/safely delete a plan
- `GET|PUT /api/v1/routing` global routing policy
- `POST /api/v1/routing/bootstrap` initialize global ACTIVE / STANDBY nodes
- `GET|PUT|PATCH /api/v1/settings/providers` read, full update (`PUT`), or partial update (`PATCH`) of provider/probe/policy/weekly-optimizer settings; stored secrets are never returned
- `POST /api/v1/settings/providers/test/chmlfrp` read-only ChmlFrp credential/connectivity test; auto-renews via saved SSO credentials
- `POST /api/v1/settings/providers/chmlfrp/sso/login` trigger ChmlFrp Qzhua OAuth2 SSO login with saved/supplied username, password, and Base32 TOTP 2FA secret
- `POST /api/v1/settings/providers/test/cloudflare` verify Cloudflare token and optionally read the selected zone's A records
- `POST /api/v1/settings/providers/cloudflare/zones` verify a Cloudflare token and list accessible zones
- `GET|POST /api/v1/chmlfrp/tunnels` list/create unmanaged ChmlFrp tunnels
- `PUT|DELETE /api/v1/chmlfrp/tunnels/{id}` update unmanaged ChmlFrp tunnel (`DELETE` returns 409 due to v2 API restriction)
- `POST /api/v1/chmlfrp/tunnels/{id}/test-write` safe round-trip write test on an unmanaged tunnel
- `POST /api/v1/chmlfrp/diagnostics` ChmlFrp read + config-generation diagnostics
- `GET|POST /api/v1/dns/records` list/create Cloudflare DNS records
- `PUT|DELETE /api/v1/dns/records/{id}` update/delete unmanaged Cloudflare DNS records
- `POST /api/v1/dns/diagnostics` Cloudflare temporary TXT CRUD + cleanup diagnostics
- `GET /api/v1/nodes` ChmlFrp nodes with real `frps_port`, `web_supported`, `tags`, `policy`, telemetry (`load1/5/15`, `bandwidth_usage_percent`), `notes`, and quarantine state
- `POST /api/v1/nodes/probe` concurrently TCP-probe online non-quarantined ChmlFrp nodes on their real `frps_port` and return RTTs, `require_web`, `web_supported`, `fastest`, `recommended`, `eligible`, and `ineligible_reasons`
- `POST /api/v1/nodes/switch-fastest` probe compliant nodes and switch only if a faster node beats active by `min_improvement_ms` (accepts optional `{ dry_run, min_improvement_ms }`; skips without interrupting FRPC if active is already optimal)
- `POST /api/v1/nodes/{name}/unquarantine` clear quarantine eligibility only; never auto-failback
- `POST /api/v1/reconcile` global desired-state reconciliation
- `POST /api/v1/failover` manual global failover
- `GET /api/v1/frpc/status`
- `GET /api/v1/frpc/logs`
- `POST /api/v1/frpc/start|stop|restart`
- `GET /api/v1/events` FRPC Server-Sent Events (`?api_key=<API_KEY>`)

Once an Active node exists, `PUT /routing` cannot directly replace it. Node changes must use `GLOBAL_FAILOVER` or `POST /api/v1/nodes/switch-fastest`.

## Provider settings semantics

`PUT /api/v1/settings/providers` replaces the full settings document; `PATCH /api/v1/settings/providers` updates only the fields provided (ideal for AI/scripts). Omitted/blank secrets keep the stored values. `clear_chmlfrp_token` / `clear_cloudflare_api_token` / `clear_chmlfrp_credentials` explicitly remove secrets. The response exposes only boolean configuration flags. Provider writes return `409` while global reconcile/failover is running.
