#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export DATA_DIR="${DATA_DIR:-$ROOT/data}"
export PUBLIC_DIR="${PUBLIC_DIR:-$ROOT/public}"
export HTTP_PORT="${HTTP_PORT:-8080}"
export FRPC_BINARY_PATH="${FRPC_BINARY_PATH:-$(command -v frpc || true)}"
export FRPC_CONFIG_PATH="${FRPC_CONFIG_PATH:-$DATA_DIR/frpc/conf/frpc.toml}"
export FRPC_BACKUP_DIR="${FRPC_BACKUP_DIR:-$DATA_DIR/backups/frpc}"
export FRPC_LOG_PATH="${FRPC_LOG_PATH:-$DATA_DIR/frpc/logs/frpc.log}"
if [[ -z "$FRPC_BINARY_PATH" || ! -x "$FRPC_BINARY_PATH" ]]; then
  echo "未找到可执行 frpc。请安装 frpc，或设置 FRPC_BINARY_PATH。" >&2
  exit 1
fi
mkdir -p "$(dirname "$FRPC_CONFIG_PATH")" "$FRPC_BACKUP_DIR" "$(dirname "$FRPC_LOG_PATH")"
exec node --experimental-strip-types "$ROOT/src/server/index.ts"
