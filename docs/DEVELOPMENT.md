# Development

Run the complete local quality gate:

```bash
bash scripts/check.sh
```

CI generates a dependency lock, normalizes Rust formatting in the ephemeral CI workspace, runs tests with `--locked`, runs Clippy, and builds the deployable server as static `x86_64-unknown-linux-musl`. The browser JavaScript is syntax-checked with Node but is not compiled or bundled.

The only automatic GitHub workflow is `.github/workflows/ci.yml`. Pull requests run quality/build only. A successful `main` build uploads the verified runtime bundle; the publish job downloads exactly that bundle and packages/pushes GHCR. Docker does not recompile Rust.

Do not introduce per-tunnel node fields, per-tunnel failover handlers, or a second automatic build workflow.


## Dependency updates

Dependabot is configured for Cargo, GitHub Actions and the Docker base image. Keep dependency updates in focused pull requests and preserve the product-invariant checklist in the PR template.


## Cargo.lock

This repository is an application and should ultimately commit `Cargo.lock`. The 0.2.0 refactor was produced in an environment without a runnable Rust toolchain, so the source package cannot honestly include a newly resolved lock file. CI therefore runs `cargo generate-lockfile` before all Rust checks, uses `--locked` afterwards, and uploads the exact generated `Cargo.lock` as `cargo-lock-<sha>`. After the first green 0.2.0 run, download that artifact and commit it; subsequent dependency changes should update the committed lock in focused pull requests.
