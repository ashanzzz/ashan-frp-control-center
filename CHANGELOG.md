# Changelog

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
