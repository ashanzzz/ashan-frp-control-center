#!/bin/bash

chmlfrp_init_shared_layout() {
  local default_log_dir="$1"
  local base_dir="${BASE_DIR:-$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)}"

  LOG_DIR="${LOG_DIR:-$default_log_dir}"
  [ -n "$LOG_DIR" ] || LOG_DIR="$base_dir"

  SETTINGS_FILE="${SETTINGS_FILE:-$LOG_DIR/settings.env}"
  USERDATA_FILE="${USERDATA_FILE:-$LOG_DIR/userdata.txt}"
  FIXED_TUNNEL_FILE="${FIXED_TUNNEL_FILE:-$LOG_DIR/fixed_tunnels.txt}"
  EXEMPT_NAMES_FILE="${EXEMPT_NAMES_FILE:-$LOG_DIR/exempt_names.txt}"

  STATUS_FILE="${STATUS_FILE:-$LOG_DIR/chmlfrp-health-status.txt}"
  NODE_ALL_FILE="${NODE_ALL_FILE:-$LOG_DIR/chmlfrp-nodes-all.txt}"
  NODE_FILE="${NODE_FILE:-$LOG_DIR/chmlfrp-nodes-filtered.txt}"
  USERINFO_FILE="${USERINFO_FILE:-$LOG_DIR/chmlfrp-userinfo.txt}"

  LOCK_FILE="${LOCK_FILE:-$LOG_DIR/.controller.lock}"
  COOLDOWN_FILE="${COOLDOWN_FILE:-$LOG_DIR/chmlfrp-last-switch-state.txt}"
  BAN_FILE="${BAN_FILE:-$LOG_DIR/chmlfrp-ban-state.txt}"
  NODE_REFRESH_FILE="${NODE_REFRESH_FILE:-$LOG_DIR/chmlfrp-node-refresh-state.txt}"
  NODES_METRICS_FILE="${NODES_METRICS_FILE:-$LOG_DIR/chmlfrp-node-metrics.txt}"

  SYNC_ISSUES_FILE="${SYNC_ISSUES_FILE:-$LOG_DIR/chmlfrp-sync-issues.json}"
  TUNNEL_NAME_OVERRIDES_FILE="${TUNNEL_NAME_OVERRIDES_FILE:-$LOG_DIR/chmlfrp-tunnel-name-overrides.json}"
  NODE_ISSUES_FILE="${NODE_ISSUES_FILE:-$LOG_DIR/chmlfrp-node-issues.txt}"
  SYNC_RESULT_FILE="${SYNC_RESULT_FILE:-$LOG_DIR/chmlfrp-sync-result.txt}"

  COOLDOWN_SECONDS="${COOLDOWN_SECONDS:-900}"
  BAN_SECONDS="${BAN_SECONDS:-3600}"
  NODE_UNUSABLE_BAN_SECONDS="${NODE_UNUSABLE_BAN_SECONDS:-2592000}"
  NODE_PROXY_CONFLICT_THRESHOLD="${NODE_PROXY_CONFLICT_THRESHOLD:-3}"
  POST_RESTART_SETTLE_SECONDS="${POST_RESTART_SETTLE_SECONDS:-8}"
  POST_RESTART_LOG_TAIL="${POST_RESTART_LOG_TAIL:-120}"
}

chmlfrp_now_ts() {
  date +%s
}

chmlfrp_mask_token() {
  local token="$1"
  if [ -z "$token" ] || [ "$token" = "null" ]; then
    echo "<empty>"
    return 0
  fi
  printf '%s***' "${token:0:4}"
}

chmlfrp_load_settings() {
  if [ -f "$SETTINGS_FILE" ]; then
    set -a
    # shellcheck disable=SC1090
    . "$SETTINGS_FILE"
    set +a
  fi
}

chmlfrp_write_json_file() {
  local file="$1"
  local content="$2"
  local tmp="${file}.tmp"
  printf '%s\n' "$content" > "$tmp" && mv "$tmp" "$file"
}

chmlfrp_json_ok() {
  local file="$1"
  [ -f "$file" ] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  jq empty "$file" >/dev/null 2>&1
}

chmlfrp_json_get() {
  local file="$1"
  local query="$2"
  jq -r "$query" "$file" 2>/dev/null
}

chmlfrp_init_json_file() {
  local file="$1"
  local default_json="$2"
  if [ ! -f "$file" ] || ! chmlfrp_json_ok "$file"; then
    chmlfrp_write_json_file "$file" "$default_json"
  fi
}

chmlfrp_write_status() {
  local status="$1"
  local reason="$2"
  local details="$3"
  local proxy_status="${4:-unknown}"
  local proxy_reason="${5:-unknown}"
  local payload
  payload=$(jq -n \
    --arg status "$status" \
    --arg reason "$reason" \
    --arg details "$details" \
    --arg proxy_status "$proxy_status" \
    --arg proxy_reason "$proxy_reason" \
    --argjson ts "$(chmlfrp_now_ts)" \
    '{status:$status, reason:$reason, details:$details, proxy_status:$proxy_status, proxy_reason:$proxy_reason, ts:$ts}')
  chmlfrp_write_json_file "$STATUS_FILE" "$payload"
}

chmlfrp_read_status_field() {
  local field="$1"
  local default_value="${2:-unknown}"
  if [ ! -f "$STATUS_FILE" ]; then
    printf '%s' "$default_value"
    return 0
  fi
  if chmlfrp_json_ok "$STATUS_FILE"; then
    chmlfrp_json_get "$STATUS_FILE" ".${field} // \"${default_value}\""
  else
    grep -oE '"'"$field"'"\s*:\s*"[^"]+"' "$STATUS_FILE" | head -n 1 | sed -E 's/.*"'"$field"'"\s*:\s*"([^"]+)".*/\1/'
  fi
}

chmlfrp_init_ban_file() {
  chmlfrp_init_json_file "$BAN_FILE" '{banned:[]}'
}

chmlfrp_is_banned() {
  local name="$1"
  chmlfrp_json_ok "$BAN_FILE" || return 1
  local now
  now=$(chmlfrp_now_ts)
  jq -e --arg n "$name" --argjson now "$now" '(.banned//[]) | map(select(.name==$n and (.until//0)>$now)) | length>0' "$BAN_FILE" >/dev/null 2>&1
}

chmlfrp_record_node_issue() {
  local node_name="$1"
  local classification="$2"
  local detail="$3"
  local ban_until="${4:-0}"

  command -v jq >/dev/null 2>&1 || return 0
  chmlfrp_init_json_file "$NODE_ISSUES_FILE" '{records:[]}'
  local tmp="${NODE_ISSUES_FILE}.tmp"
  jq --arg n "$node_name" \
     --arg c "$classification" \
     --arg d "$detail" \
     --argjson until "$ban_until" \
     --argjson ts "$(chmlfrp_now_ts)" \
     '.records += [{node:$n, classification:$c, detail:$d, until:$until, ts:$ts}]' \
     "$NODE_ISSUES_FILE" > "$tmp" && mv "$tmp" "$NODE_ISSUES_FILE"
}

chmlfrp_ban_node_for() {
  local name="$1"
  local reason="$2"
  local duration_seconds="$3"
  local classification="${4:-node_ban}"
  local detail="${5:-}"

  command -v jq >/dev/null 2>&1 || return 0
  chmlfrp_init_ban_file
  local until
  until=$(( $(chmlfrp_now_ts) + duration_seconds ))

  jq --arg n "$name" \
     --arg r "$reason" \
     --arg c "$classification" \
     --arg d "$detail" \
     --argjson u "$until" '
    .banned = ((.banned // []) | map(select(.name != $n)) + [{name:$n, reason:$r, classification:$c, detail:$d, until:$u}])
  ' "$BAN_FILE" > "${BAN_FILE}.tmp" && mv "${BAN_FILE}.tmp" "$BAN_FILE"

  chmlfrp_record_node_issue "$name" "$classification" "$detail" "$until"
}

chmlfrp_write_sync_result() {
  local status="$1"
  local classification="$2"
  local message="$3"
  local node_name="$4"
  local detail="${5:-}"

  command -v jq >/dev/null 2>&1 || return 0
  local payload
  payload=$(jq -n \
    --arg status "$status" \
    --arg classification "$classification" \
    --arg message "$message" \
    --arg node "$node_name" \
    --arg detail "$detail" \
    --argjson ts "$(chmlfrp_now_ts)" \
    '{status:$status, classification:$classification, message:$message, node:$node, detail:$detail, ts:$ts}')
  chmlfrp_write_json_file "$SYNC_RESULT_FILE" "$payload"
}

chmlfrp_read_sync_result_field() {
  local field="$1"
  local default_value="${2:-}"
  if [ ! -f "$SYNC_RESULT_FILE" ]; then
    printf '%s' "$default_value"
    return 0
  fi
  if chmlfrp_json_ok "$SYNC_RESULT_FILE"; then
    chmlfrp_json_get "$SYNC_RESULT_FILE" ".${field} // \"${default_value}\""
  else
    printf '%s' "$default_value"
  fi
}
