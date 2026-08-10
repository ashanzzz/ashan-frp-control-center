#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
SERVER="${SERVER_BIN:-target/release/ashan-frp-server}"
OUT="${RELEASE_DIR:-.release}"

test -f "$SERVER" || { echo "missing $SERVER" >&2; exit 1; }
test -f web/index.html || { echo "missing web/index.html" >&2; exit 1; }
rm -rf "$OUT"
mkdir -p "$OUT/web" "$OUT/unraid"
cp "$SERVER" "$OUT/ashan-frp-server"
chmod 0755 "$OUT/ashan-frp-server"
cp -a web/. "$OUT/web/"
cp README.md CHANGELOG.md compose.yaml .env.example "$OUT/"
cp unraid/ashan-frp-control-center.xml "$OUT/unraid/"
