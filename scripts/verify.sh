#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/verify.py
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  git diff --check
fi
if command -v cargo >/dev/null 2>&1; then
  cargo test --workspace --exclude ashan-frp-web
  cargo check --workspace --exclude ashan-frp-web
else
  echo 'cargo not found: skipped Rust compile checks' >&2
fi
if command -v dx >/dev/null 2>&1; then
  dx build --release --platform web
else
  echo 'dx not found: skipped Dioxus build check' >&2
fi
