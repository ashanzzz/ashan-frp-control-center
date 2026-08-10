#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
python3 scripts/verify.py
cargo test --workspace --exclude ashan-frp-web
cargo check --workspace --exclude ashan-frp-web
cargo clippy --workspace --exclude ashan-frp-web --all-targets
bash scripts/build-web.sh
cargo build --release -p ashan-frp-server
bash scripts/stage-release.sh
