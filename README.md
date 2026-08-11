# Ashan FRP Control Center

Self-hosted control plane for **ChmlFrp + FRPC + Cloudflare DNS** with node-level high availability.

[![CI](https://github.com/ashanzzz/ashan-frp-control-center/actions/workflows/ci.yml/badge.svg)](https://github.com/ashanzzz/ashan-frp-control-center/actions/workflows/ci.yml)

## Product contract

The following rules are architectural invariants:

- All managed tunnels share **one global active ChmlFrp node**.
- There is **no per-tunnel node assignment and no per-tunnel failover**.
- ChmlFrp is the tunnel configuration source and generates the complete FRPC configuration.
- FRPC is the runtime and primary fault-signal source.
- A confirmed remote/node-side FRPC failure from any tunnel triggers **one GLOBAL_FAILOVER for all managed tunnels**.
- Local-service, authentication and local-configuration faults do not trigger node failover.
- Confirmed failed nodes are quarantined for 30 days by default; quarantine is configurable.
- Cloudflare A records are changed **only after the target ChmlFrp configuration has been loaded and all planned FRPC tunnels have passed runtime verification**.
- Only A records explicitly managed by this application are modified.
- Automatic failback is not performed when a quarantine expires.

## Architecture

```text
Tunnel plans (source of truth)
        │
        ├── ChmlFrp API ── all tunnels on one active node
        │                       │
        │                       └── generated full frpc config
        │                                      │
        ├──────────────────────────────────── FRPC
        │                                      │
        │                               runtime logs/events
        │                                      │
        └── Global Failover Coordinator <──────┘
                         │
                         └── Cloudflare A records (last stage)
```

The backend is Rust/Axum/Tokio/SQLx/SQLite. The WebUI is plain static HTML, CSS and ES modules served by Axum. There is no Node.js, Vue, React, Dioxus, WASM or frontend build toolchain.

## Container deployment

```bash
cp .env.example .env
docker compose up -d
```

No provider token is required to start the container. Open **WebUI → Settings** after startup:

1. Enter the ChmlFrp API token, click **Test connection**, then save it. The shared client is reconfigured immediately; no Docker restart is required.
2. Enter a scoped Cloudflare API token, click **Verify token / Load zones**, select the zone, run the read-only Token + Zone test, then save it.
3. Choose the initial global Active/Standby nodes and save the routing policy. After an Active node exists, later Active-node changes must use **GLOBAL_FAILOVER**.

`CHMLFRP_*` and `CLOUDFLARE_*` environment variables remain optional one-shot compatibility seeds. They are imported only on the first v0.3+ startup; after that, SQLite/WebUI is authoritative. Even an explicit **Clear Token** remains cleared after later container restarts.

Persistent state is under `/data`, including the SQLite provider settings/secrets. For Unraid the template maps it to:

```text
/mnt/cache/appdata/ashan-frp-control-center -> /data
```

Place the Linux amd64 `frpc` executable at:

```text
/data/frpc/frpc
```

and make it executable. ChmlFrp remains responsible for generating the configuration used at `/data/frpc/frpc.ini`.

## Provider authorization and connection tests

The Settings page never returns a saved provider token to the browser. Password fields are blank after save and only show whether a token is configured. A new non-empty token replaces the stored value; explicit **Clear Token** actions remove it. Provider changes are blocked while a global reconcile/failover operation owns the coordinator lock.

ChmlFrp documents a browser authorization-login flow for its own clients. The current public material does not document third-party OAuth client registration and a callback URI for self-hosted control panels, so this project does **not** reuse or impersonate ChmlFrp's own OAuth client. The WebUI instead offers a direct link to the official ChmlFrp panel for account/authorization/token management and uses the supported API token for control-plane access.

Cloudflare testing is non-destructive: the WebUI verifies the API token, lists accessible zones and reads A records for the selected zone. It does not create a temporary record just to prove DNS Edit permission.

## Development

Requirements: Rust 1.94.1+, Python 3, Node.js only for `node --check` of the browser JavaScript (Node is **not** a runtime dependency).

```bash
bash scripts/check.sh
```

The CI quality gate resolves one dependency lock, runs repository invariants, JavaScript syntax validation, Rust formatting/parsing, all tests with `--locked`, Clippy, and one static musl linux/amd64 release server build. Only the verified runtime artifact is passed to the GHCR publish job.

## Repository layout

```text
apps/server/        Axum API, coordinator, reconciliation and static hosting
crates/domain/      Shared domain types and routing state machine types
crates/database/    SQLite persistence and migrations
crates/chmlfrp/     ChmlFrp v2 adapter
crates/cloudflare/  Cloudflare DNS adapter
crates/frpc-log/    FRPC log classifier
crates/frpc-runtime/Managed FRPC child process and readiness verification
crates/failover/    Eligible-node ordering
web/                Static operations console
migrations/         SQLite migrations
unraid/             Unraid Docker template
scripts/            Verification and release staging
docs/               Architecture and operations documentation
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Global failover](docs/FAILOVER.md)
- [API](docs/API.md)
- [Operations](docs/OPERATIONS.md)
- [Development](docs/DEVELOPMENT.md)
- [0.2.0 engineering review](docs/REVIEW.md)
- [Security](SECURITY.md)

## Important ChmlFrp boundaries

- Emergency failover intentionally does not provision missing tunnels. Before a node switch, every enabled tunnel plan must already exist remotely; use **Global Reconcile** to create/repair missing resources. This prevents an emergency failover from becoming a partially provisioned mixed-state operation.
- The runtime always consumes the complete configuration generated by ChmlFrp for the selected node. The control center never synthesizes per-tunnel FRPC configuration.
- Safe local deletion is intentionally blocked while the matching ChmlFrp tunnel or managed Cloudflare record still exists. This prevents the source-of-truth row from disappearing while externally managed resources remain orphaned.

## License

MIT.
