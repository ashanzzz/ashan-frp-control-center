# Changelog

## 0.4.1 - 2026-11-26

- Add real-port node intelligence (`frps_port` from `/nodeinfo`, `notes`, `nodegroup`, `china`, `fangyu`, `load1/5/15`, `bandwidth_usage_percent`) and structured capability/stability tags (`可建站`, `大带宽`/`AS4837`/`10G`, `禁网盘/大流量`, `被攻击/不可用`, `高负载`, `需备案`, `需过白`, `封海外`, `有防御`), plus migration `0006_node_policy_settings.sql` for latency-first + drive/bandwidth-compatible candidate filtering.
- Add multi-method ChmlFrp authentication via Qzhua OAuth2 SSO + RFC 6238 TOTP 2FA (`POST /api/v1/settings/providers/chmlfrp/sso/login`) and automatic `/sso/refresh` renewal with `Authorization: Bearer` API alignment (plus migration `0005_chmlfrp_multi_auth.sql`).
- Add `crates/node-probe` for concurrent TCP RTT probing across eligible ChmlFrp nodes (3 connect attempts per node, averaged RTT, configurable port/timeout/tolerance).
- Integrate latency-first ranking (`sort_by_latency`) into global failover after filtering by online status, quarantine, and `web_supported` (when any enabled HTTP/HTTPS tunnel exists).
- Protect preferred `STANDBY` priority within the configured tolerance band (`probe_latency_tolerance_ms`, default `50 ms`), while automatically promoting faster nodes beyond tolerance.
- Abort failover cleanly with an explicit Activity error if every candidate node times out on the probe port.
- Add migration `0004_probe_settings.sql` storing `probe_port` (default `7000`), `probe_timeout_ms` (default `2000`), and `probe_latency_tolerance_ms` (default `50`) in SQLite `provider_settings`.
- Add `POST /api/v1/nodes/probe` endpoint and WebUI controls: "测量全部节点" button + "探针延迟" column on the ChmlFrp page, and "节点延迟探针" settings card in Settings.

## 0.4.0 - 2026-11-15

- Add Provider Resource Console (ChmlFrp tunnels/nodes, Cloudflare DNS records CRUD & diagnostics, unmanaged resource import/unmanage, and initial global routing bootstrap).
- Display live `Connected` / `Disconnected` / `需要 Zone` / `未配置` health badges instead of static `Configured`.

## 0.3.0 - 2026-08-10

- Move ChmlFrp and Cloudflare provider credentials into WebUI-managed SQLite settings under `/data`; Docker environment variables are now optional one-shot bootstrap seeds only, so a WebUI-cleared token cannot reappear after restart.
- Hot-reconfigure both provider clients after a settings save, so token/base URL/Zone changes take effect without restarting the container.
- Add read-only provider tests: ChmlFrp token/connectivity, Cloudflare token verification, accessible Zone discovery, and selected-zone A-record reads.
- Never return saved provider token values through the settings API; expose only configured/not-configured booleans and explicit clear-token actions.
- Block provider configuration writes while global reconcile/failover owns the coordinator operation lock.
- Add a direct link to the official ChmlFrp panel/authorization flow while deliberately avoiding undocumented third-party OAuth client reuse.
- Simplify the Unraid template: only WebUI Port and AppData are essential visible fields; provider/routing environment variables become optional advanced bootstrap settings.
- Preserve the single global Active-node / GLOBAL_FAILOVER architecture and Cloudflare-last failover ordering.

## 0.2.2 - 2026-08-10

- Make CI source-preserving: `cargo fmt --all -- --check`, strict Clippy, committed `Cargo.lock`, and a final clean-worktree assertion.
- Refresh GitHub Actions to current Node 24-era majors (`checkout@v7`, Buildx/Login v4, Build/Push v7).
- Keep one CI workflow and one job; pull requests validate only while `main` publishes GHCR.
- Keep Dependabot as one weekly multi-ecosystem PR and restrict Cargo automation to lockfile-only updates.
- Align workspace package and OCI image metadata on version 0.2.2.

## 0.2.1 - 2026-08-10

### CI/CD and Build Cleanups
- Fix `apps/server/src/state.rs` `parse_env` compilation error by mapping `FromStr::Err` explicitly to `anyhow::Error`.
- Commit authoritative `Cargo.lock` to Git repository.
- Consolidate GitHub Actions CI workflow into a single job using `cargo fmt --all -- --check`.
- Configure Dependabot `multi-ecosystem-groups` to consolidate Cargo, GitHub Actions, and Docker updates into single weekly PRs.

## 0.2.0 - 2026-08-10

### Architecture
- Replace the Dioxus/WASM frontend with a static HTML/CSS/ES-module operations console served directly by Axum.
- Remove the frontend compiler/WASM target from the release path; the application now has one Rust server build plus repository static assets.
- Package one verified static linux/amd64 musl server artifact into GHCR; Docker no longer recompiles Rust or frontend code.
- Add standard repository metadata: EditorConfig, Dependabot, pull-request template, bug-report form, contributing guide and security policy.

### Reliability
- Introduce typed `RoutingPhase` values and typed target-node migration errors instead of free-form/string-prefix control flow.
- Stop the confirmed failed active FRPC runtime before automatic candidate migration and invalidate its runtime generation immediately.
- Preserve Forward Recovery: only confirmed node failures quarantine a candidate; provider/configuration errors do not.
- Remove a duplicate manual WebUI failover invocation so one click can create only one global failover operation.
- Add Provider HTTP connect/request timeouts and Cloudflare A-record pagination across the complete result set.
- Remove a preflight `expect` panic path when remote ChmlFrp state changes during migration.
- Add graceful HTTP shutdown followed by managed FRPC shutdown.
- Add manual quarantine clearing without automatic failback.

### Data and operations
- Add migration `0002_tunnel_plan_constraints.sql` with protocol/port/boolean constraints and a partial unique index for managed DNS domains.
- Allow multiple TCP/UDP plans without DNS to use an empty domain safely.
- Distinguish Provider API outages from genuinely missing ChmlFrp/DNS resources in the dashboard.
- Make successful Global Reconcile converge recoverable routing phases back to `idle`.
- Refresh Docker Compose, Unraid template, README and operator/developer documentation for the 0.2.0 architecture.

### Preserved invariants
- Every managed tunnel uses one global active node; there is no per-tunnel failover.
- Any confirmed node-side FRPC fault triggers one `GLOBAL_FAILOVER` for all managed tunnels.
- ChmlFrp remains the complete FRPC configuration source.
- Managed Cloudflare A records remain the final failover stage after target FRPC verification.
- Quarantine expiry/clearing never causes automatic failback.

## 0.1.x

Initial Rust prototype and iterative CI/failover hardening. The 0.2.0 release supersedes the Dioxus build architecture used by 0.1.x.
