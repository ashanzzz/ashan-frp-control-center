# HTTP API

All JSON success responses use `{ "data": ... }`. Errors use `{ "code": "...", "message": "..." }`.

- `GET /api/v1/health` liveness and version
- `GET /api/v1/ready` SQLite readiness
- `GET /api/v1/dashboard` aggregated control-plane snapshot
- `GET|POST /api/v1/tunnels` list/create plans
- `PUT|DELETE /api/v1/tunnels/{id}` update/safely delete a plan
- `GET|PUT /api/v1/routing` global routing policy
- `GET /api/v1/nodes` ChmlFrp nodes with quarantine state
- `POST /api/v1/nodes/{name}/unquarantine` clear quarantine eligibility only; never auto-failback
- `POST /api/v1/reconcile` global desired-state reconciliation
- `POST /api/v1/failover` manual global failover
- `GET /api/v1/frpc/status`
- `GET /api/v1/frpc/logs`
- `POST /api/v1/frpc/start|stop|restart`
- `GET /api/v1/events` FRPC Server-Sent Events

Once an Active node exists, `PUT /routing` cannot directly replace it. Node changes must use global failover.
