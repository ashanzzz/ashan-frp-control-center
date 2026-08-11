# Development

Run the complete local quality gate:

```bash
bash scripts/check.sh
```

CI treats the committed `Cargo.lock` and Rust sources as immutable inputs. It checks formatting with `cargo fmt --all -- --check`, runs tests and strict Clippy with `--locked`, then builds the deployable server as static `x86_64-unknown-linux-musl`. The browser JavaScript is syntax-checked with Node but is not compiled or bundled.

The only automatic GitHub workflow is `.github/workflows/ci.yml`. Pull requests run quality/build only. A successful `main` run packages the same verified `.release` runtime into GHCR; Docker does not recompile Rust.

Do not introduce per-tunnel node fields, per-tunnel failover handlers, or a second automatic build workflow.


## Dependency updates

Dependabot groups Cargo, GitHub Actions and Docker updates into one weekly pull request. Cargo uses `lockfile-only`, so routine automation refreshes compatible resolved dependencies without rewriting manifest requirements across breaking pre-1.0 version boundaries. Manifest dependency upgrades should be reviewed explicitly.

## Cargo.lock

`Cargo.lock` is committed because this repository ships an application. All CI and release commands use `--locked`; dependency changes must update the lockfile in the same focused change.
