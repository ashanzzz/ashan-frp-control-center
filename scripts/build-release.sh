#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
./scripts/build-web.sh
cargo build --release -p ashan-frp-server
./scripts/stage-release.sh
