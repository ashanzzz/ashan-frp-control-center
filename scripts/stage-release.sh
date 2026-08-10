#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SERVER="target/release/ashan-frp-server"
WEB="dist/public"
OUT=".release"

test -x "$SERVER" || { echo "ERROR: missing executable $SERVER" >&2; exit 1; }
test -f "$WEB/index.html" || { echo "ERROR: missing $WEB/index.html" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT/web"
cp "$SERVER" "$OUT/ashan-frp-server"
cp -a "$WEB"/. "$OUT/web"/

# Include install/deployment metadata in downloadable artifacts; Docker only
# consumes the server and web directory.
cp README.md CHANGELOG.md compose.yaml .env.example "$OUT"/
mkdir -p "$OUT/unraid"
cp unraid/ashan-frp-control-center.xml "$OUT/unraid"/
