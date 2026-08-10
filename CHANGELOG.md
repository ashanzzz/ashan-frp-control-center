# Changelog

## 0.1.1 - 2026-08-09

- Hardened FRPC log classification and runtime-generation isolation.
- Automatic node faults now wait for the global operation lock instead of being dropped.
- Candidate nodes are quarantined only after confirmed node-scoped runtime failures.
- Active-node truth is committed after FRPC validation and before DNS; DNS failures become `degraded_dns`.
- Reconcile preflights Cloudflare/node state, verifies remote ChmlFrp convergence, and avoids unnecessary FRPC restarts.
- Tunnel-plan deletion is guarded against remote ChmlFrp/DNS orphan creation.
- FRPC automatically restores from the persisted ChmlFrp-generated config after container restart.
- Duplicate managed Cloudflare A records are surfaced and block reconcile/failover preflight.
- Added CI, downloadable Linux release artifact workflow, and quality-gated GHCR image publishing.
- Added `docs/LOGIC_REVIEW.md`.


## 0.1.0 - 2026-08-09

- Greenfield Rust rewrite of Ashan FRP Control Center.
- Dioxus Web/WASM control console and Axum API server.
- SQLite/SQLx desired-state storage with one singleton global routing state.
- ChmlFrp adapter for tunnel/node/config operations with legacy/current response aliases.
- FRPC process runtime and structured log classifier.
- Node-scoped `GLOBAL_FAILOVER`: no per-tunnel failover path exists.
- Configurable failed-node quarantine, default 30 days.
- Forward Recovery for target nodes proven bad by FRPC startup/runtime verification.
- Cloudflare A records updated only after all target FRPC tunnels become ready.
- Docker, Compose, Unraid template, CI and GHCR publishing workflow.
