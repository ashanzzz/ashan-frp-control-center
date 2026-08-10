# Changelog

## v0.1.5 - 2026-08-09

- Fix Dioxus workspace build ambiguity by making `--package ashan-frp-web` mandatory through `scripts/build-web.sh`.
- Pin the Dioxus CLI to 0.7.9 and make the web package selection explicit for a deterministic frontend build entry point.
- Refactor CI/CD so Web and Server are compiled exactly once; Docker is now runtime-only and packages `.release/`.
- Keep a single GitHub Actions workflow; PR builds validate the runtime image without pushing, while main pushes publish GHCR.
- Strengthen `scripts/verify.py` so bare `dx build` and build-stage Dockerfiles cannot reappear.

## v0.1.4

- Fixed Axum 0.8 server compilation by importing the `put` routing function used by the tunnel update route.
- Consolidated GitHub Actions into one authoritative `.github/workflows/ci.yml` pipeline.
- Removed the duplicate `build.yml` and `build-push.yml` workflows.
- Main pushes now run exactly one workflow: verify -> test -> check -> clippy -> web build -> server build -> artifact -> GHCR.
- Pull requests run the same quality/build path but never publish images.
- Tag pushes no longer trigger a second duplicate build.

## v0.1.3

- Fix reqwest 0.13 `query` feature required by ChmlFrp and Cloudflare adapters.
- Upgrade `actions/checkout` to v6 (Node 24 runtime).
- Re-run CI-focused source review.

# Changelog

## 0.1.2 - 2026-08-09

- 完全重建 Unraid Docker XML 模板，删除旧模板内容。
- 默认使用 Bridge 网络、单 8080 WebUI、非特权运行，不挂载 Docker Socket。
- AppData 固定推荐 `/mnt/cache/appdata/ashan-frp-control-center -> /data`。
- Active/Standby Node 改为可选首次启动种子，避免安装时强制填写。
- 增加 ChmlFrp/Cloudflare API Base、FRPC 日志缓存与 Rust 日志等级高级选项。
- 模板说明明确全局节点模型：任何确认的远端节点故障只允许触发 ALL-TUNNELS `GLOBAL_FAILOVER`。

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
