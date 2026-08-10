#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v dx >/dev/null 2>&1; then
  echo "ERROR: Dioxus CLI (dx) is required." >&2
  exit 127
fi

# Single authoritative Dioxus build command for this workspace.
# The workspace has two binaries (server + web), therefore the web package must
# always be explicit. Do not replace this with a bare `dx build`.
dx build --release --platform web --package ashan-frp-web

test -f dist/public/index.html || {
  echo "ERROR: Dioxus build completed without dist/public/index.html" >&2
  exit 1
}
