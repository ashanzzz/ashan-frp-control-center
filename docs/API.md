# HTTP API

All JSON success responses use `{ "data": ... }`. Errors use `{ "code": "...", "message": "..." }`.

- `GET /api/v1/health` liveness and version
- `GET /api/v1/ready` SQLite readiness
- `GET /api/v1/dashboard` aggregated control-plane snapshot
- `GET|POST /api/v1/tunnels` list/create plans
- `PUT|DELETE /api/v1/tunnels/{id}` update/safely delete a plan
- `GET|PUT /api/v1/routing` global routing policy
- `GET|PUT /api/v1/settings/providers` read/update provider configuration; stored secrets are never returned
- `POST /api/v1/settings/providers/test/chmlfrp` read-only ChmlFrp credential/connectivity test; supplied unsaved token takes precedence
- `POST /api/v1/settings/providers/test/cloudflare` verify Cloudflare token and optionally read the selected zone's A records
- `POST /api/v1/settings/providers/cloudflare/zones` verify a Cloudflare token and list accessible zones
- `GET /api/v1/nodes` ChmlFrp nodes with quarantine state
- `POST /api/v1/nodes/{name}/unquarantine` clear quarantine eligibility only; never auto-failback
- `POST /api/v1/reconcile` global desired-state reconciliation
- `POST /api/v1/failover` manual global failover
- `GET /api/v1/frpc/status`
- `GET /api/v1/frpc/logs`
- `POST /api/v1/frpc/start|stop|restart`
- `GET /api/v1/events` FRPC Server-Sent Events

Once an Active node exists, `PUT /routing` cannot directly replace it. Node changes must use global failover.

## Provider settings semantics

Provider update payloads contain base URLs, the selected Cloudflare Zone ID and optional replacement tokens. An omitted/blank token keeps the stored secret. `clear_chmlfrp_token` / `clear_cloudflare_api_token` explicitly remove a secret. The response exposes only whether each token is configured. Provider writes return `409` while global reconcile/failover is running.
