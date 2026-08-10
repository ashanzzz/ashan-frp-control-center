# Engineering review for 0.2.0

## Why 0.1.x repeatedly failed to publish

The backend workspace had already reached the point where its Rust tests and checks could pass, but the release path still depended on a second compiled frontend toolchain: Dioxus CLI plus a WASM target. Small UI ownership/type errors therefore blocked the entire container release after the backend had already compiled.

0.2.0 removes that failure domain. The operations console is static HTML/CSS/ES modules served by Axum. There is no frontend compiler, no WASM target, and no Node runtime.

## Logic review changes

The 0.2.0 review also hardened behavior that is independent of the build system:

- Provider-unavailable state is distinct from a missing tunnel/DNS record.
- Routing phases are typed instead of free-form strings.
- Target-node runtime failures are typed instead of detected by string prefixes.
- A confirmed automatic active-node failure stops the old FRPC runtime before candidate migration.
- A failed candidate FRPC runtime is stopped before forward recovery continues.
- Manual failover restores the original healthy node before trying another candidate, preventing a later rollback from restoring a quarantined candidate.
- Successful Global Reconcile always returns routing to `idle`, including recovery from a previous `failed` phase.
- DNS duplicate-record conflicts are detected before ChmlFrp mutation, and Cloudflare A-record discovery follows all result pages instead of assuming the first page is complete.
- Provider requests have finite connect/request timeouts so the global operation lock cannot hang indefinitely on a half-open HTTP request.
- SQLite uses WAL, a busy timeout, explicit constraints, and a migration that permits multiple non-DNS TCP/UDP plans with an empty domain.
- Quarantine can be cleared manually without automatic failback.
- SIGTERM/CTRL-C performs graceful HTTP shutdown and then stops the managed FRPC child.
- Stopping FRPC immediately increments the runtime generation, invalidating buffered logs from the stopped process before any candidate-node work begins.
- Automatic failover never restores a runtime already confirmed bad; non-node migration failures stop FRPC and leave the operation in a controlled failed state.
- Provider snapshot construction no longer contains a panic path if remote resources change between preflight and mutation.
- The manual WebUI failover action emits exactly one `POST /api/v1/failover`; a duplicate invocation found during the 0.2.0 review was removed and covered by a repository invariant check.

## Build and release model

There is one automatic workflow: `.github/workflows/ci.yml`.

The quality job resolves one `Cargo.lock`, validates repository invariants, browser JavaScript syntax, Rust formatting/parsing, tests and Clippy with the locked dependency graph, and performs one static musl linux/amd64 release build of `ashan-frp-server`. It then stages the exact server binary plus static WebUI into `.release/`.

The publish job consumes only that uploaded `.release/` artifact and packages it into the GHCR runtime image. Docker does not run Cargo and cannot silently compile a second, different artifact.

## Remaining operational boundary

The WebUI currently has no built-in authentication. Run it only on a trusted LAN/VPN or behind an existing authenticated reverse proxy. This is intentional and documented rather than hidden behind a false security assumption.

The first green 0.2.0 run also uploads the generated `Cargo.lock` so it can be committed back to the application repository.
