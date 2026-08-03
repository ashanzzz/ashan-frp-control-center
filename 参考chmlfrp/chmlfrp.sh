#!/bin/bash
set -o pipefail

###############################################################################
# Unraid + ChmlFrp 全自动控制脚本（单文件入口）
#
# 设计目标（科学化）：
# - 可靠判定：健康检查独立产出状态 JSON（online/offline + reason）
# - 智能决策：离线才 failover；fastest 带冷却避免抖动；失败节点 ban；多节点回退
# - 单行可用：适配 Unraid CA User Scripts（一个计划任务一行命令）
# - 不改 ChmlFrp API：仍使用既有 endpoints（login/node/nodeinfo/tunnel_config 等由 new_fix_flow 执行）
#
# 你只需要管理 2 个脚本：
# - chmlfrp.sh（本控制器）
# - new_fix_flow.sh（执行"全量同步到指定节点 + 重建 frpc 容器"）
#
# ─── API 文档与版本说明 ─────────────────────────────────────────────────────
# 官方 API 文档: https://docs.chmlfrp.net/API/v2/
# Apifox 在线文档: https://s.apifox.cn/24b31bd1-e48b-44ab-a486-81cf5f964422
#
# V2 API（cf-v2.uapis.cn）— 主力使用：
#   GET  /login?username=&password=     → 登录，获取 token
#   GET  /node                          → 节点列表（无参数，返回所有节点基本信息）
#   GET  /nodeinfo?token=&node=         → 节点详情（含 state/realIp/ip/port 等）
#   GET  /tunnel?token=                 → 隧道列表
#   POST /create_tunnel                 → 创建隧道
#   GET  /tunnel_config?token=&node=    → 生成 frpc 配置
#
# ⚠️  V2 删除隧道接口 /delete_tunnel 官方标注"还在开发中，暂时无法使用"
#     文档: https://docs.chmlfrp.net/API/v2/Tunnel_operations/delete_tunnel.html
#
# V1 API（cf-v1.uapis.cn）— 仅用于删除隧道：
#   GET  /api/deletetl.php?token=&userid=&nodeid=  → 删除隧道
#     文档: https://docs.chmlfrp.net/API/v1/Tunnel_operations/deletetl.html
#     注意: V1 官方已标注"即将被放弃使用"，但删除隧道目前只能用 V1
###############################################################################
# Unraid + ChmlFrp 全自动控制脚本（单文件入口）
#
# 设计目标（科学化）：
# - 可靠判定：健康检查独立产出状态 JSON（online/offline + reason）
# - 智能决策：离线才 failover；fastest 带冷却避免抖动；失败节点 ban；多节点回退
# - 单行可用：适配 Unraid CA User Scripts（一个计划任务一行命令）
# - 不改 ChmlFrp API：仍使用既有 endpoints（login/node/nodeinfo/tunnel_config 等由 new_fix_flow 执行）
#
# 你只需要管理 2 个脚本：
# - chmlfrp.sh（本控制器）
# - new_fix_flow.sh（执行“全量同步到指定节点 + 重建 frpc 容器”）
###############################################################################

# --- 基本路径：默认以脚本所在目录作为 LOG_DIR（推荐把脚本放在 chmlfrp 日志目录） ---
BASE_DIR="$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)"
SHARED_LIB="$BASE_DIR/chmlfrp_shared.sh"
[ -f "$SHARED_LIB" ] || SHARED_LIB="${LOG_DIR:-$BASE_DIR}/chmlfrp_shared.sh"
[ -f "$SHARED_LIB" ] || { echo "[ERROR] 缺少共享库 chmlfrp_shared.sh" >&2; exit 1; }
# shellcheck disable=SC1090
. "$SHARED_LIB"
chmlfrp_init_shared_layout "$BASE_DIR"
LOG_DIR="${LOG_DIR:-$BASE_DIR}"

# --- 配置文件（建议与脚本同目录） ---
SETTINGS_FILE="${SETTINGS_FILE:-$LOG_DIR/settings.env}"
USERDATA_FILE="${USERDATA_FILE:-$LOG_DIR/userdata.txt}"
FIXED_TUNNEL_FILE="${FIXED_TUNNEL_FILE:-$LOG_DIR/fixed_tunnels.txt}"
EXEMPT_NAMES_FILE="${EXEMPT_NAMES_FILE:-$LOG_DIR/exempt_names.txt}"

# --- 输出/状态文件（统一使用 .txt 扩展名，内容为 JSON 结构） ---
STATUS_FILE="${STATUS_FILE:-$LOG_DIR/chmlfrp-health-status.txt}"
NODE_ALL_FILE="${NODE_ALL_FILE:-$LOG_DIR/chmlfrp-nodes-all.txt}"
NODE_FILE="${NODE_FILE:-$LOG_DIR/chmlfrp-nodes-filtered.txt}"
USERINFO_FILE="${USERINFO_FILE:-$LOG_DIR/chmlfrp-userinfo.txt}"

# --- frpc ---
FRPC_DOCKER_NAME="${FRPC_DOCKER_NAME:-frpc}"
FRPC_CONFIG_PATH="${FRPC_CONFIG_PATH:-/mnt/user/appdata/frpc/frpc.toml}"

# --- 节点筛选策略（可在 settings.env 覆盖） ---
FILTER_CHINA="${FILTER_CHINA:-all}"        # yes/no/all
FILTER_TYPE="${FILTER_TYPE:-all}"          # vip/user/all
FILTER_BUILD_SITE="${FILTER_BUILD_SITE:-all}" # yes/no/all
FILTER_NOTES="${FILTER_NOTES:-}"           # 关键字，逗号分隔；空=不过滤

# --- 自动化策略（可在 settings.env 覆盖） ---
NODE_REFRESH_SECONDS="${NODE_REFRESH_SECONDS:-3600}"
COOLDOWN_SECONDS="${COOLDOWN_SECONDS:-900}"
BAN_SECONDS="${BAN_SECONDS:-3600}"
MAX_TRIES="${MAX_TRIES:-3}"

PING_ATTEMPTS="${PING_ATTEMPTS:-5}"
PING_TIMEOUT="${PING_TIMEOUT:-2}"
MIN_SUCCESS="${MIN_SUCCESS:-3}"
SLEEP_BETWEEN="${SLEEP_BETWEEN:-0.2}"

# 可选：端到端健康检查 URL（例如 https://xxx/health）
HEALTHCHECK_URL="${HEALTHCHECK_URL:-}"

# --- QZhua OAuth2 配置 ---
QZHUA_API_BASE="${QZHUA_API_BASE:-https://account-api.qzhua.net}"
QZHUA_TOKEN_ENDPOINT="${QZHUA_TOKEN_ENDPOINT:-${QZHUA_API_BASE}/oauth2/token}"
QZHUA_DEVICE_CODE_ENDPOINT="${QZHUA_DEVICE_CODE_ENDPOINT:-${QZHUA_API_BASE}/oauth2/device_authorization}"
QZHUA_SCOPE="${QZHUA_SCOPE:-chmlfrp_api}"
TOKEN_EXPIRE_BUFFER="${TOKEN_EXPIRE_BUFFER:-60}"

# --- 内部文件 ---
LOCK_FILE="$LOG_DIR/.controller.lock"
COOLDOWN_FILE="$LOG_DIR/chmlfrp-last-switch-state.txt"
BAN_FILE="$LOG_DIR/chmlfrp-ban-state.txt"
NODE_REFRESH_FILE="$LOG_DIR/chmlfrp-node-refresh-state.txt"
NODES_METRICS_FILE="$LOG_DIR/chmlfrp-node-metrics.txt"

now_ts() { chmlfrp_now_ts; }
log() { echo "[$(date '+%F %T')][$1] $2"; }
info() { log INFO "$1"; }
warn() { log WARNING "$1"; }
err() { log ERROR "$1"; }
success() { log SUCCESS "$1"; }

mask_token() { chmlfrp_mask_token "$1"; }

api_request_log() {
  local tag="$1" method="$2" url="$3" auth="$4"
  if [ -n "$auth" ]; then
    info "[API][$tag][REQUEST] method=$method url=$url auth=$auth"
  else
    info "[API][$tag][REQUEST] method=$method url=$url"
  fi
}

api_response_log() {
  local tag="$1" resp="$2"
  if [ -z "$resp" ]; then
    warn "[API][$tag][RESPONSE] empty"
    return 0
  fi
  if echo "$resp" | jq empty >/dev/null 2>&1; then
    local code state msg
    code=$(echo "$resp" | jq -r '.code // .success // "n/a"')
    state=$(echo "$resp" | jq -r '.state // "n/a"')
    msg=$(echo "$resp" | jq -r '.msg // .error // (.errors[0].message // "")')
    info "[API][$tag][RESPONSE] code=$code state=$state msg=$msg"
  else
    local preview
    preview=$(printf '%s' "$resp" | tr '\r\n' ' ' | cut -c 1-300)
    warn "[API][$tag][RESPONSE] non_json preview=$preview"
  fi
}

require_cmd() { command -v "$1" >/dev/null 2>&1; }

load_settings() { chmlfrp_load_settings; }

with_lock() {
  mkdir -p "$LOG_DIR" >/dev/null 2>&1 || true
  if require_cmd flock; then
    exec 200>"$LOCK_FILE"
    if ! flock -n 200; then
      warn "已有任务在运行，退出（lock=$LOCK_FILE）"
      exit 0
    fi
  else
    warn "系统未找到 flock，无法加锁（可能发生并发执行）"
  fi
}

json_ok() { chmlfrp_json_ok "$1"; }

json_get() { chmlfrp_json_get "$1" "$2"; }

write_json_file() { chmlfrp_write_json_file "$1" "$2"; }

write_status() { chmlfrp_write_status "$1" "$2" "$3" "$4" "$5"; }

parse_toml_kv() {
  local file="$1" key_regex="$2"
  grep -E "$key_regex" "$file" 2>/dev/null | head -n 1 | sed -E 's/^[[:space:]]*[^=]+=[[:space:]]*"?([^"#]+)"?.*$/\1/'
}

tcp_probe() {
  local host="$1" port="$2" timeout_secs="${3:-2}"
  require_cmd timeout || return 2
  timeout "$timeout_secs" bash -c "cat < /dev/null > /dev/tcp/${host}/${port}" >/dev/null 2>&1
}

http_probe() {
  local url="$1" timeout_secs="${2:-3}"
  [ -z "$url" ] && return 2
  require_cmd curl || return 2
  curl -fsS --max-time "$timeout_secs" "$url" >/dev/null 2>&1
}

health_check() {
  # 1) docker
  if ! require_cmd docker; then
    write_status offline docker_missing "docker 命令不存在"
    return 0
  fi
  if ! docker ps -a --format '{{.Names}}' | grep -wq "$FRPC_DOCKER_NAME"; then
    write_status offline container_missing "frpc 容器不存在"
    return 0
  fi
  local running
  running=$(docker inspect -f '{{.State.Running}}' "$FRPC_DOCKER_NAME" 2>/dev/null || echo false)
  if [ "$running" != "true" ]; then
    write_status offline container_not_running "frpc 容器未运行"
    return 0
  fi

  # 2) frpc.toml
  if [ ! -f "$FRPC_CONFIG_PATH" ]; then
    write_status offline frpc_config_missing "frpc.toml 不存在: $FRPC_CONFIG_PATH" unknown ""
    return 0
  fi
  local server_addr server_port
  server_addr=$(parse_toml_kv "$FRPC_CONFIG_PATH" '^[[:space:]]*(server_addr|serverAddr)[[:space:]]*=')
  server_port=$(parse_toml_kv "$FRPC_CONFIG_PATH" '^[[:space:]]*(server_port|serverPort)[[:space:]]*=')
  if [ -z "$server_addr" ]; then
    write_status offline frpc_config_invalid "frpc.toml 未找到 server_addr" unknown ""
    return 0
  fi
  if [ -n "$server_port" ]; then
    if ! tcp_probe "$server_addr" "$server_port" 2; then
      write_status offline tcp_connect_fail "TCP 连接失败: ${server_addr}:${server_port}" unknown ""
      return 0
    fi
  fi

  # 3) 可选端到端
  if [ -n "$HEALTHCHECK_URL" ]; then
    if ! http_probe "$HEALTHCHECK_URL" 3; then
      write_status offline http_healthcheck_fail "HTTP 健康检查失败: $HEALTHCHECK_URL" unknown ""
      return 0
    fi
  fi

  # 4) 日志关键错误
  local logs
  logs=$(docker logs --tail 80 "$FRPC_DOCKER_NAME" 2>/dev/null || true)
  if echo "$logs" | grep -q "start error: 客户端代理参数错误"; then
    write_status online ok "container running; config ok; probes ok; proxy issue: config_mismatch" degraded config_mismatch
    return 0
  fi
  if echo "$logs" | grep -Eqi "router config conflict|route config conflict|proxy \[.*\] already exists"; then
    write_status degraded server_node_conflict "container running; config ok; probes ok; proxy issue: server_node_conflict" degraded server_node_conflict
    return 0
  fi
  if echo "$logs" | grep -Eqi "(connect to server.*failed|login to server failed|i/o timeout|connection refused|no route to host)"; then
    write_status offline server_connect_fail "检测到疑似服务端连接失败日志" unknown ""
    return 0
  fi

  write_status online ok "container running; config ok; probes ok" ok ok
  return 0
}

read_status() {
  chmlfrp_read_status_field status unknown
}

read_proxy_status() {
  chmlfrp_read_status_field proxy_status unknown
}

read_proxy_reason() {
  chmlfrp_read_status_field proxy_reason unknown
}

read_status_reason() {
  chmlfrp_read_status_field reason unknown
}

status_requires_manual_fix() {
  local reason="$1"
  case "$reason" in
    config_mismatch)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

proxy_requires_switch() {
  local proxy_reason="$1"
  case "$proxy_reason" in
    server_node_conflict|node_unusable|router_conflict|node_proxy_conflict|node_proxy_already_exists)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

cooldown_ok() {
  [ -f "$COOLDOWN_FILE" ] || return 0
  local last now diff
  if json_ok "$COOLDOWN_FILE"; then
    last=$(json_get "$COOLDOWN_FILE" '.ts // 0')
  else
    last=0
  fi
  now=$(now_ts)
  diff=$(( now - last ))
  [ "$diff" -ge "$COOLDOWN_SECONDS" ]
}

mark_switched() {
  require_cmd jq || return 0
  write_json_file "$COOLDOWN_FILE" "$(jq -n --argjson ts "$(now_ts)" '{ts:$ts}')"
}

init_ban_file() {
  chmlfrp_init_ban_file
}

is_banned() {
  chmlfrp_is_banned "$1"
}

ban_node() {
  local name="$1" reason="$2"
  chmlfrp_ban_node_for "$name" "$reason" "$BAN_SECONDS" short_ban "ban_seconds=$BAN_SECONDS"
}

ban_node_long() {
  local name="$1" reason="$2" detail="$3"
  chmlfrp_ban_node_for "$name" "$reason" "$NODE_UNUSABLE_BAN_SECONDS" node_unusable "$detail"
}

urlencode() {
  local s="$1"
  curl -s -o /dev/null -w '%{url_effective}' --get --data-urlencode "name=$s" 'http://dummy' | sed 's/.*name=//g'
}

userinfo_sync() {
  require_cmd curl || { err "缺少 curl"; return 1; }
  require_cmd jq || { err "缺少 jq"; return 1; }

  if ! json_ok "$USERDATA_FILE"; then
    err "userdata.txt（内部为 JSON）不存在或非法：$USERDATA_FILE"
    return 1
  fi
  local url resp code access_token u p

  if is_oauth2_enabled; then
    access_token=$(get_access_token)
    if [ -z "$access_token" ]; then
      err "无法获取 access_token，无法同步用户详情"
      return 1
    fi
    url="https://cf-v2.uapis.cn/login?access_token=${access_token}"
    api_request_log "login" "GET" "https://cf-v2.uapis.cn/login?access_token=$(mask_token "$access_token")" "oauth2_access_token"
  else
    u=$(json_get "$USERDATA_FILE" '.chmlfrp.username // empty')
    p=$(json_get "$USERDATA_FILE" '.chmlfrp.password // empty')
    if [ -z "$u" ] || [ -z "$p" ]; then
      err "userdata.txt（JSON）缺少 chmlfrp.username/chmlfrp.password（用于 login）"
      return 1
    fi
    url="https://cf-v2.uapis.cn/login?username=${u}&password=<masked>"
    api_request_log "login" "GET" "$url" "username_password"
    url="https://cf-v2.uapis.cn/login?username=${u}&password=${p}"
  fi

  resp=$(curl -sS -L --connect-timeout 5 --max-time 15 "$url" || true)
  api_response_log "login" "$resp"
  if [ -z "$resp" ] || ! echo "$resp" | jq empty >/dev/null 2>&1; then
    err "login API 返回空或非 JSON"
    return 1
  fi
  code=$(echo "$resp" | jq -r '.code // 0')
  if [ "$code" -ne 200 ]; then
    err "login 失败：code=$code msg=$(echo "$resp" | jq -r '.msg // ""')"
    return 1
  fi
  echo "$resp" | jq '.' > "$USERINFO_FILE"
  success "已写入用户详情：$USERINFO_FILE"
  return 0
}

ensure_userinfo() {
  # 确保 USERINFO_FILE 存在且合法，否则尝试同步一次
  if json_ok "$USERINFO_FILE"; then
    return 0
  fi
  info "用户详情缺失或非法，尝试自动同步：$USERINFO_FILE"
  userinfo_sync || true
  json_ok "$USERINFO_FILE"
}

node_refresh_needed() {
  local now last diff threshold
  now=$(now_ts)
  threshold="$NODE_REFRESH_SECONDS"
  if json_ok "$NODE_REFRESH_FILE"; then
    last=$(json_get "$NODE_REFRESH_FILE" '.ts // 0')
  else
    last=0
  fi
  diff=$(( now - last ))
  if [ "$diff" -ge "$threshold" ]; then
    info "[NODE][REFRESH] action=refresh_all_nodes reason=cache_expired age=${diff}s threshold=${threshold}s"
    return 0
  fi
  if ! json_ok "$NODE_FILE"; then
    info "[NODE][REFRESH] action=refresh_all_nodes reason=filtered_cache_missing_or_invalid age=${diff}s threshold=${threshold}s"
    return 0
  fi
  info "[NODE][REFRESH] action=use_cached_nodes reason=cache_fresh age=${diff}s threshold=${threshold}s"
  return 1
}

node_list_refresh() {
  # 生成两个缓存文件：
  # 1) NODE_ALL_FILE      - 全量节点快照
  # 2) NODE_FILE          - 按 settings.env 过滤后的候选节点快照
  require_cmd curl || { err "缺少 curl"; return 1; }
  require_cmd jq || { err "缺少 jq"; return 1; }

  info "[NODE][REFRESH] begin action=refresh_all_nodes"
  local node_api_url="https://cf-v2.uapis.cn/node"
  api_request_log "node_list" "GET" "$node_api_url" "none"
  local resp
  resp=$(curl -sS -L --connect-timeout 5 --max-time 15 "$node_api_url" || true)
  api_response_log "node_list" "$resp"
  if [ -z "$resp" ] || ! echo "$resp" | jq empty >/dev/null 2>&1; then
    err "node API 返回空或非 JSON"
    return 1
  fi
  if [ "$(echo "$resp" | jq -r '.code // 0')" != "200" ]; then
    err "node API 失败：code=$(echo "$resp" | jq -r '.code // 0')"
    return 1
  fi

  local refreshed_at
  refreshed_at=$(now_ts)
  info "[NODE][FILTER] china=${FILTER_CHINA} type=${FILTER_TYPE} build_site=${FILTER_BUILD_SITE} notes=${FILTER_NOTES:-<empty>}"
  info "[NODE][REFRESH] note=/node only provides node metadata; real IP for ping is resolved later via nodeinfo"

  # --- 输出 API 返回的全部原始节点 ---
  local total_raw
  total_raw=$(echo "$resp" | jq '.data | length')
  info "[NODE][REFRESH] api_total=$total_raw"
  info "API 返回全部节点（共 $total_raw 个）："
  echo "$resp" | jq -r '.data[] | "  [\(.nodegroup)] \(.name) | 地区:\(.area) | 中国:\(.china) | 建站:\(.web)"' | while IFS= read -r line; do
    info "$line"
  done

  write_json_file "$NODE_ALL_FILE" "$(echo "$resp" | jq --argjson ts "$refreshed_at" --arg src "$node_api_url" '
    {
      refreshed_at: $ts,
      source: $src,
      total: (.data | length),
      nodes: (.data | map({
        id,
        name,
        area,
        realIp: (.realIp // ""),
        ip: (.ip // ""),
        nodegroup,
        china,
        web,
        udp,
        fangyu,
        notes: (.notes // ""),
        ipv6
      }))
    }
  ')"

  # 过滤策略：基于列表字段（china/nodegroup/web/notes）
  local notes_filter_json
  if [ -n "$FILTER_NOTES" ]; then
    # 转成数组
    notes_filter_json=$(printf '%s' "$FILTER_NOTES" | jq -R 'split(",") | map(gsub("^\\s+|\\s+$";"")) | map(select(length>0))')
  else
    notes_filter_json='[]'
  fi

  write_json_file "$NODE_FILE" "$(echo "$resp" | jq --argjson ts "$refreshed_at" --arg src "$NODE_ALL_FILE" --arg fc "$FILTER_CHINA" --arg ft "$FILTER_TYPE" --arg fw "$FILTER_BUILD_SITE" --argjson fn "$notes_filter_json" '
    def match_yes_no_all(v; req):
      if req=="all" then true
      elif req=="yes" then (v=="yes" or v==true)
      elif req=="no" then (v=="no" or v==false)
      else true end;
    def match_group(v; req):
      if req=="all" then true else (v==req) end;
    def match_notes(n; arr):
      if (arr|length)==0 then true
      else (n // "") as $note | any(arr[]; $note | tostring | contains(.)) end;

    .data as $all
    | ($all
      | map(select(match_yes_no_all(.china; $fc)))
      | map(select(match_group(.nodegroup; $ft)))
      | map(select(match_yes_no_all(.web; $fw)))
      | map(select(match_notes(.notes; $fn)))) as $filtered
    | {
        refreshed_at: $ts,
        source_file: $src,
        filters: {
          china: $fc,
          type: $ft,
          build_site: $fw,
          notes: $fn
        },
        total_before: ($all | length),
        total_after: ($filtered | length),
        nodes: ($filtered | map({
          "节点名称": .name,
          "节点本地IPv4": (.realIp // ""),
          "节点地区": .area,
          "权限组": .nodegroup,
          "是否中国节点": .china,
          "是否支持建站": .web,
          "节点介绍": (.notes // "")
        }))
      }
  ')"

  # --- 输出筛选详情 ---
  local total_before total_after
  total_before=$(echo "$resp" | jq '.data | length')
  total_after=$(jq '.nodes | length' "$NODE_FILE")

  # 构建筛选条件描述
  local filters=""
  [ "$FILTER_CHINA" != "all" ] && filters+=" 中国=$FILTER_CHINA"
  [ "$FILTER_TYPE" != "all" ] && filters+=" 权限组=$FILTER_TYPE"
  [ "$FILTER_BUILD_SITE" != "all" ] && filters+=" 建站=$FILTER_BUILD_SITE"
  [ -n "$FILTER_NOTES" ] && filters+=" 关键字=$FILTER_NOTES"
  [ -z "$filters" ] && filters="无（全部节点）"

  info "[NODE][REFRESH] filtered_total=$total_after total_before=$total_before"
  info "节点筛选完成：原始 $total_before 个 → 筛选后 $total_after 个（条件:$filters）"

  # 列出筛选后的节点详情
  if [ "$total_after" -gt 0 ]; then
    while IFS= read -r line; do
      [ -n "$line" ] && info "$line"
    done < <(jq -r '.nodes[] | "[NODE][MATCH] name=\(."节点名称" // "") cacheIp=\((."节点本地IPv4" // "") | if .=="" then "none" else . end) area=\(."节点地区" // "") group=\(."权限组" // "") china=\(."是否中国节点" // "") web=\(."是否支持建站" // "") notes=\((."节点介绍" // "") | if .=="" then "none" else . end)"' "$NODE_FILE")
  else
    warn "[NODE][FILTER] no nodes matched current filters"
    warn "筛选后节点列表为空！请检查筛选条件是否过于严格"
  fi

  write_json_file "$NODE_REFRESH_FILE" "$(jq -n \
    --argjson ts "$refreshed_at" \
    --arg all_file "$NODE_ALL_FILE" \
    --arg filtered_file "$NODE_FILE" \
    --argjson total_before "$total_before" \
    --argjson total_after "$total_after" \
    '{ts:$ts, all_nodes_file:$all_file, filtered_nodes_file:$filtered_file, total_before:$total_before, total_after:$total_after}')"
  info "[NODE][REFRESH] completed refreshed_at=$refreshed_at filtered_file=$NODE_FILE all_file=$NODE_ALL_FILE"
  success "节点全量缓存已写入：$NODE_ALL_FILE"
  success "节点筛选缓存已写入：$NODE_FILE"
  return 0
}

nodeinfo_get() {
  local token="$1" name="$2"
  local enc url
  enc=$(urlencode "$name")
  url="https://cf-v2.uapis.cn/nodeinfo?token=${token}&node=${enc}"
  curl -sS -L --connect-timeout 3 --max-time 6 "$url"
}

# 网络连通性诊断：测试多个外网地址，判断是 token 问题还是网络问题
network_diagnosis() {
  info "=== 网络连通性诊断 ==="
  local all_ok=true

  # 测试 ChmlFrp API 服务器
  if curl -fsS --max-time 5 -o /dev/null "https://cf-v2.uapis.cn/" 2>/dev/null; then
    info "  ✅ ChmlFrp API (cf-v2.uapis.cn) 可达"
  else
    warn "  ❌ ChmlFrp API (cf-v2.uapis.cn) 不可达"
    all_ok=false
  fi

  # 测试国内常用站点
  local test_sites=(
    "https://www.baidu.com|百度"
    "https://www.bing.com|Bing"
    "https://www.weibo.com|微博"
    "https://www.cctv.com|CCTV"
  )

  for site_entry in "${test_sites[@]}"; do
    local url="${site_entry%%|*}"
    local label="${site_entry##*|}"
    if curl -fsS --max-time 5 -o /dev/null "$url" 2>/dev/null; then
      info "  ✅ $label ($url) 可达"
    else
      warn "  ❌ $label ($url) 不可达"
      all_ok=false
    fi
  done

  if $all_ok; then
    info "=== 网络连通性正常，问题出在 token 认证（可能已过期或无效）==="
  else
    warn "=== 部分外网不可达，请检查本机网络/DNS 设置 ==="
  fi
}

ping_quality() {
  # 输出："avg_ms\tloss_pct\treceived"
  local target="$1"
  local out
  out=$(ping -c "$PING_ATTEMPTS" -W "$PING_TIMEOUT" "$target" 2>/dev/null || true)
  [ -n "$out" ] || return 1

  # 解析收包
  local received
  received=$(echo "$out" | awk -F',' '/packets transmitted/ {gsub(/^[[:space:]]+/,"",$2); print $2}' | awk '{print $1}' | head -n 1)
  [ -n "$received" ] || received=0
  if [ "$received" -lt "$MIN_SUCCESS" ]; then
    return 1
  fi

  # 解析丢包百分比
  local loss
  loss=$(echo "$out" | awk -F',' '/packet loss/ {print $3}' | sed -E 's/[^0-9.]+//g' | head -n 1)
  [ -n "$loss" ] || loss=100

  # 解析 avg RTT
  local avg
  avg=$(echo "$out" | sed -nE 's/^(rtt|round-trip)[^=]*= ([0-9.]+)\/([0-9.]+)\/.*/\3/p' | head -n 1)
  if [ -z "$avg" ]; then
    # 退化：从 time= 的行做粗略平均
    avg=$(echo "$out" | awk -F'time=' '/time=/{print $2}' | awk '{print $1}' | awk '{s+=$1; c++} END{if(c>0) printf("%.0f", s/c); else print ""}')
  fi
  [ -n "$avg" ] || return 1

  printf '%.0f\t%.0f\t%s\n' "$avg" "$loss" "$received"
  return 0
}

select_best_node() {
  # 注意：本函数会被 auto_failover/auto_fastest 通过 $() 调用，
  # 所有日志必须走 stderr（>&2），否则会污染命令替换捕获的输出。
  # 唯一走 stdout 的是最终的候选行（score<TAB>name<TAB>...）。
  _sbn_log() { echo "[$(date '+%F %T')][$1] $2" >&2; }
  _sbn_api_request() {
    local tag="$1" method="$2" url="$3" auth="$4"
    if [ -n "$auth" ]; then
      _sbn_log INFO "[API][$tag][REQUEST] method=$method url=$url auth=$auth"
    else
      _sbn_log INFO "[API][$tag][REQUEST] method=$method url=$url"
    fi
  }
  _sbn_api_response() {
    local tag="$1" resp="$2"
    if [ -z "$resp" ]; then
      _sbn_log WARNING "[API][$tag][RESPONSE] empty"
      return 0
    fi
    if echo "$resp" | jq empty >/dev/null 2>&1; then
      local code state msg
      code=$(echo "$resp" | jq -r '.code // .success // "n/a"')
      state=$(echo "$resp" | jq -r '.state // .data.state // "n/a"')
      msg=$(echo "$resp" | jq -r '.msg // .error // ""')
      _sbn_log INFO "[API][$tag][RESPONSE] code=$code state=$state msg=$msg"
    else
      local preview
      preview=$(printf '%s' "$resp" | tr '\r\n' ' ' | cut -c 1-300)
      _sbn_log WARNING "[API][$tag][RESPONSE] non_json preview=$preview"
    fi
  }

  require_cmd jq || { _sbn_log ERROR "缺少 jq"; return 1; }
  require_cmd curl || { _sbn_log ERROR "缺少 curl"; return 1; }

  if ! json_ok "$USERDATA_FILE"; then
    _sbn_log ERROR "userdata.txt（内部为 JSON）不存在或非法：$USERDATA_FILE"
    return 1
  fi
  local token
  token=$(get_access_token)
  if [ -z "$token" ]; then
    _sbn_log ERROR "无法获取有效的 access_token"
    return 1
  fi
  if ! json_ok "$NODE_FILE"; then
    _sbn_log ERROR "节点文件不存在或非法：$NODE_FILE"
    return 1
  fi

  local total
  total=$(jq '.nodes | length' "$NODE_FILE")
  [ "$total" -gt 0 ] || { _sbn_log ERROR "节点文件 nodes 为空"; return 1; }

  local candidates=()
  local metrics_tmp
  metrics_tmp="${NODES_METRICS_FILE}.tmp"
  write_json_file "$metrics_tmp" "$(jq -n --argjson ts "$(now_ts)" '{generated_at:$ts, entries:[]}')"
  local i
  _sbn_log INFO "开始遍历 $total 个节点进行测速选优..."
  local auth_error_count=0
  for ((i=0; i<total; i++)); do
    local name ip_from_file
    name=$(jq -r ".nodes[$i][\"节点名称\"] // \"\"" "$NODE_FILE")
    ip_from_file=$(jq -r ".nodes[$i][\"节点本地IPv4\"] // \"\"" "$NODE_FILE")
    [ -z "$name" ] && continue

    if is_banned "$name"; then
      _sbn_log INFO "跳过 ban 节点：$name"
      continue
    fi

    local resp
    local name_encoded
    name_encoded=$(urlencode "$name")
    local nodeinfo_url="https://cf-v2.uapis.cn/nodeinfo?token=${token}&node=${name_encoded}"
    _sbn_api_request "nodeinfo" "GET" "$nodeinfo_url" "query_token=$(mask_token "$token")"
    resp=$(nodeinfo_get "$token" "$name")
    _sbn_api_response "nodeinfo" "$resp"
    if [ -z "$resp" ] || ! echo "$resp" | jq empty >/dev/null 2>&1; then
      _sbn_log INFO "[NODE][SKIP] name=$name reason=nodeinfo_non_json_or_empty"
      continue
    fi
    local code state real_ip domain_ip
    code=$(echo "$resp" | jq -r '.code // 0')
    if [ "$code" -ne 200 ]; then
      # 401 = 未授权（token 无效/过期），连续出现说明不是单个节点问题
      if [ "$code" -eq 401 ]; then
        auth_error_count=$((auth_error_count + 1))
        if [ "$auth_error_count" -eq 1 ]; then
          _sbn_log WARNING "⚠️  nodeinfo 返回 code=401（未授权），token 可能已过期或无效"
          _sbn_log WARNING "提示：请运行 'chmlfrp.sh oauth_refresh' 刷新 Token"
          _sbn_log INFO "触发网络连通性诊断（判断是 token 问题还是本地网络问题）..."
          network_diagnosis >&2
        fi
        if [ "$auth_error_count" -ge 2 ]; then
          _sbn_log ERROR "连续 $auth_error_count 个节点返回 401，确认 token 已失效"
          _sbn_log ERROR "请先运行 'chmlfrp.sh oauth_refresh' 刷新 Token 后再试"
          return 1
        fi
      fi
      _sbn_log INFO "[NODE][SKIP] name=$name reason=nodeinfo_code_$code"
      continue
    fi
    state=$(echo "$resp" | jq -r '.data.state // "unknown"')
    if [ "$state" != "online" ]; then
      _sbn_log INFO "[NODE][SKIP] name=$name reason=state_$state"
      continue
    fi
    real_ip=$(echo "$resp" | jq -r '.data.realIp // ""')
    domain_ip=$(echo "$resp" | jq -r '.data.ip // ""')

    local ping_target
    ping_target="$real_ip"
    if [ -z "$ping_target" ] || [ "$ping_target" = "null" ]; then ping_target="$ip_from_file"; fi
    if [ -z "$ping_target" ] || [ "$ping_target" = "null" ]; then ping_target="$domain_ip"; fi
    if [ -z "$ping_target" ] || [ "$ping_target" = "null" ]; then
      _sbn_log INFO "[NODE][SKIP] name=$name reason=no_ping_target realIp=$real_ip cacheIp=$ip_from_file domainIp=$domain_ip"
      continue
    fi

    local pq avg loss
    pq=$(ping_quality "$ping_target" || true)
    if [ -z "$pq" ]; then
      _sbn_log INFO "[NODE][SKIP] name=$name reason=ping_failed ping_target=$ping_target"
      continue
    fi
    avg=$(printf '%s' "$pq" | cut -f1)
    loss=$(printf '%s' "$pq" | cut -f2)

    local ip_for_dns
    ip_for_dns="$real_ip"
    if [ -z "$ip_for_dns" ] || [ "$ip_for_dns" = "null" ]; then ip_for_dns="$ping_target"; fi

    # 评分：score = avg_ms + loss_pct*30
    local score
    score=$(( avg + loss * 30 ))
    _sbn_log INFO "[NODE][CANDIDATE] name=$name score=$score avg_ms=$avg loss_pct=$loss ping_target=$ping_target ip_for_dns=$ip_for_dns"
    candidates+=("${score}"$'\t'"${name}"$'\t'"${ip_for_dns}"$'\t'"${ping_target}"$'\t'"avg=${avg}ms loss=${loss}%")
    write_json_file "$metrics_tmp" "$(jq \
      --argjson ts "$(now_ts)" \
      --arg name "$name" \
      --arg ping_target "$ping_target" \
      --arg ip_for_dns "$ip_for_dns" \
      --argjson score "$score" \
      --argjson avg_ms "$avg" \
      --argjson loss_pct "$loss" \
      '.entries += [{ts:$ts, score:$score, name:$name, avg_ms:$avg_ms, loss_pct:$loss_pct, ping_target:$ping_target, ip_for_dns:$ip_for_dns}]' "$metrics_tmp")"
  done

  # 写出测速结果，便于你在 Unraid 里直接查看
  mv "$metrics_tmp" "$NODES_METRICS_FILE" 2>/dev/null || true

  [ ${#candidates[@]} -gt 0 ] || { _sbn_log ERROR "没有可用在线节点（nodeinfo online + ping 稳定）"; return 1; }

  # 按 score 升序排序，输出全部候选行（供 auto_failover 多轮回退使用）
  printf '%s\n' "${candidates[@]}" | sort -n -t $'\t' -k1,1
  return 0
}

current_node_name() {
  # 通过 frpc.toml 的 server_addr 反查当前节点名。
  # 优先级：
  # 1) 本地节点缓存（快）
  # 2) 隧道列表 API（通常最可靠，因为当前隧道天然带所属 node）
  # 3) 遍历 nodeinfo 做 IP 反查（最慢，但兜底）
  [ -f "$FRPC_CONFIG_PATH" ] || { echo ""; return 0; }
  local ip
  ip=$(parse_toml_kv "$FRPC_CONFIG_PATH" '^[[:space:]]*(server_addr|serverAddr)[[:space:]]*=')
  [ -n "$ip" ] || { echo ""; return 0; }

  if json_ok "$NODE_ALL_FILE"; then
    local name
    name=$(jq -r --arg ip "$ip" '.nodes[] | select((.realIp // "")==$ip or (.ip // "")==$ip) | .name' "$NODE_ALL_FILE" | head -n 1)
    [ -n "$name" ] && { echo "$name"; return 0; }
  fi

  if json_ok "$NODE_FILE"; then
    local name
    name=$(jq -r --arg ip "$ip" '.nodes[] | select(."节点本地IPv4"==$ip) | ."节点名称"' "$NODE_FILE" | head -n 1)
    [ -n "$name" ] && { echo "$name"; return 0; }
  fi

  local token
  token=$(get_access_token 2>/dev/null || true)
  if [ -n "$token" ]; then
    local tunnel_resp
    tunnel_resp=$(curl -sS -X GET "https://cf-v2.uapis.cn/tunnel" -H "Authorization: Bearer ${token}" 2>/dev/null || true)
    if [ -n "$tunnel_resp" ] && echo "$tunnel_resp" | jq empty >/dev/null 2>&1; then
      local tunnel_name
      tunnel_name=$(echo "$tunnel_resp" | jq -r --arg ip "$ip" '.data[]? | select((.ip // "") == $ip) | .node' | head -n 1)
      [ -n "$tunnel_name" ] && { echo "$tunnel_name"; return 0; }
    fi

    local lookup_file="$NODE_FILE"
    if ! json_ok "$lookup_file" && json_ok "$NODE_ALL_FILE"; then
      lookup_file="$NODE_ALL_FILE"
    fi
    if json_ok "$lookup_file"; then
      local total idx candidate_name resp real_ip domain_ip
      total=$(jq '.nodes | length' "$lookup_file" 2>/dev/null || echo 0)
      for ((idx=0; idx<total; idx++)); do
        candidate_name=$(jq -r ".nodes[$idx][\"节点名称\"] // .nodes[$idx].name // \"\"" "$lookup_file")
        [ -z "$candidate_name" ] && continue
        resp=$(nodeinfo_get "$token" "$candidate_name")
        [ -z "$resp" ] && continue
        echo "$resp" | jq empty >/dev/null 2>&1 || continue
        [ "$(echo "$resp" | jq -r '.code // 0')" = "200" ] || continue
        real_ip=$(echo "$resp" | jq -r '.data.realIp // ""')
        domain_ip=$(echo "$resp" | jq -r '.data.ip // ""')
        if [ "$real_ip" = "$ip" ] || [ "$domain_ip" = "$ip" ]; then
          echo "$candidate_name"
          return 0
        fi
      done
    fi
  fi

  echo ""
}

apply_switch_to_node() {
  local node_name="$1"
  local fix="$LOG_DIR/new_fix_flow.sh"
  if [ ! -f "$fix" ]; then
    # 兼容：如果 new_fix_flow.sh 不在 LOG_DIR，就用当前脚本目录
    fix="$BASE_DIR/new_fix_flow.sh"
  fi
  if [ ! -f "$fix" ]; then
    err "找不到 new_fix_flow.sh（请把它放到同目录）"
    return 1
  fi

  # 删除隧道需要 userid；尽量保证 userinfo 存在
  ensure_userinfo || warn "用户详情仍不可用：删除隧道可能失败（但仍继续尝试修复）"

  rm -f "$SYNC_RESULT_FILE" 2>/dev/null || true
  bash "$fix" --force-run --node "$node_name"
}

handle_sync_result_for_node() {
  local node_name="$1"
  local classification message detail
  classification="$(chmlfrp_read_sync_result_field classification unknown)"
  message="$(chmlfrp_read_sync_result_field message "")"
  detail="$(chmlfrp_read_sync_result_field detail "")"

  case "$classification" in
    node_proxy_conflict|node_proxy_already_exists|node_unusable|router_conflict)
      warn "节点 [$node_name] 被判定为服务端/节点侧不可用：$classification ($message)"
      ban_node_long "$node_name" "$classification" "$detail"
      return 11
      ;;
    *)
      return 0
      ;;
  esac
}

# 固定隧道文件变更必重建：做一次“同步检查”（不带 --force-run；未变更时会快速退出）
sync_fix_flow_to_node() {
  local node_name="$1"
  local force_run="${2:-false}"
  [ -n "$node_name" ] || return 0

  local fix="$LOG_DIR/new_fix_flow.sh"
  if [ ! -f "$fix" ]; then
    fix="$BASE_DIR/new_fix_flow.sh"
  fi
  if [ ! -f "$fix" ]; then
    warn "找不到 new_fix_flow.sh，跳过同步检查"
    return 0
  fi

  ensure_userinfo || warn "用户详情仍不可用：同步过程中删除隧道可能失败（但仍继续尝试）"
  if [ "$force_run" = "true" ]; then
    info "reconcile 触发强制重建：$node_name"
    bash "$fix" --force-run --node "$node_name"
  else
    bash "$fix" --node "$node_name"
  fi
}

reconcile_current_node() {
  local cur
  cur=$(current_node_name)
  if [ -z "$cur" ]; then
    warn "无法识别当前节点，跳过 reconcile"
    return 1
  fi

  info "执行当前节点 reconcile：$cur"
  sync_fix_flow_to_node "$cur" true
  local sync_rc=$?
  handle_sync_result_for_node "$cur"
  local handle_rc=$?
  if [ "$handle_rc" -eq 11 ]; then
    warn "当前节点 [$cur] 已判定为服务端/节点侧不可用，将继续换节点"
    return 11
  fi
  return "$sync_rc"
}

auto_failover() {
  with_lock
  load_settings

  if node_refresh_needed; then
    node_list_refresh || true
  fi

  # 即使在线也要做"固定隧道变更同步检查"（满足：改了 fixed_tunnels.txt 必重建）
  local cur
  cur=$(current_node_name)
  if [ -n "$cur" ]; then
    info "failover：同步检查当前节点固定隧道变更（node=$cur）"
    sync_fix_flow_to_node "$cur" || true
  else
    local cur_ip
    cur_ip=$(parse_toml_kv "$FRPC_CONFIG_PATH" '^[[:space:]]*(server_addr|serverAddr)[[:space:]]*=' 2>/dev/null || echo "")
    warn "failover：无法确定当前节点"
    warn "  原因：frpc.toml 中配置的节点 IP（$cur_ip）未在节点列表中找到"
    warn "  操作：跳过固定隧道同步检查"
    warn "  建议：运行 'chmlfrp.sh nodes' 刷新节点列表"
  fi

  health_check
  local st
  st=$(read_status)
  local reason
  reason=$(read_status_reason)
  local proxy_status proxy_reason
  proxy_status=$(read_proxy_status)
  proxy_reason=$(read_proxy_reason)
  local need_switch=0
  if proxy_requires_switch "$proxy_reason"; then
    need_switch=1
  fi
  info "当前状态：$st"
  if [ "$proxy_status" != "ok" ] && [ "$proxy_status" != "unknown" ]; then
    warn "代理状态：$proxy_status（$proxy_reason）"
    if [ -n "$cur" ]; then
      info "代理层降级，先对当前节点执行 reconcile"
      reconcile_current_node
      local reconcile_rc=$?
      health_check
      st=$(read_status)
      reason=$(read_status_reason)
      proxy_status=$(read_proxy_status)
      proxy_reason=$(read_proxy_reason)
      info "reconcile 后状态：$st"
      if [ "$reconcile_rc" -eq 11 ]; then
        warn "当前节点已判定为节点侧不可用，继续尝试其他节点"
        need_switch=1
      elif [ "$proxy_status" != "ok" ] && [ "$proxy_status" != "unknown" ]; then
        case "$proxy_reason" in
          server_node_conflict|node_unusable|router_conflict)
            warn "reconcile 后仍是节点侧冲突（$proxy_reason），继续换节点"
            need_switch=1
            ;;
          *)
            warn "reconcile 后代理状态仍为：$proxy_status（$proxy_reason）"
            return 1
            ;;
        esac
      fi
    else
      return 1
    fi
  fi
  if status_requires_manual_fix "$reason"; then
    warn "检测到配置类问题（reason=$reason），停止自动切换；请先修正固定隧道/路由配置"
    return 1
  fi
  if [ "$st" = "online" ] && [ "$need_switch" -ne 1 ]; then
    info "在线，failover 不动作"
    return 0
  fi
  if [ "$st" = "online" ] && [ "$need_switch" -eq 1 ]; then
    warn "在线但检测到节点侧冲突（proxy_reason=$proxy_reason），继续切换其他节点"
  fi

  init_ban_file

  # 一次性选出所有候选节点（按 score 升序排列）
  # select_best_node 内部日志走 stderr，stdout 只输出候选行
  local all_candidates
  all_candidates=$(select_best_node || true)
  if [ -z "$all_candidates" ]; then
    err "无法选出任何候选节点（所有节点均不可用），failover 退出"
    return 1
  fi

  local total_candidates
  total_candidates=$(echo "$all_candidates" | wc -l | tr -d ' ')
  info "选出 $total_candidates 个候选节点，将依次尝试切换..."

  local tried=0
  while IFS=$'\t' read -r score name ip_for_dns ping_target dbg; do
    [ -z "$name" ] && continue

    # 防御性校验：score 必须为纯数字
    if ! [[ "$score" =~ ^[0-9]+$ ]]; then
      warn "候选行格式异常（score 非数字）: [$score\t$name]，跳过"
      continue
    fi

    tried=$((tried + 1))
    info "尝试 #$tried/$total_candidates：切换到节点=$name (score=$score, $dbg)"

    apply_switch_to_node "$name"
    local switch_rc=$?
    if [ "$switch_rc" -ne 0 ]; then
      handle_sync_result_for_node "$name"
      local handle_rc=$?
      if [ "$handle_rc" -eq 11 ]; then
        continue
      fi
      warn "切换执行失败：$name"
      ban_node "$name" fix_flow_failed
      continue
    fi
    mark_switched

    health_check
    st=$(read_status)
    reason=$(read_status_reason)
    proxy_status=$(read_proxy_status)
    proxy_reason=$(read_proxy_reason)
    info "切换后状态：$st"
    if [ "$proxy_status" != "ok" ] && [ "$proxy_status" != "unknown" ]; then
      warn "切换后代理状态：$proxy_status（$proxy_reason）"
    fi
    if status_requires_manual_fix "$reason"; then
      warn "切换后检测到配置类问题（reason=$reason），停止继续切换"
      return 1
    fi
    if [ "$st" = "online" ]; then
      success "自愈成功：$name"
      return 0
    fi
    warn "切换后仍离线，ban 并回退：$name"
    ban_node "$name" post_switch_offline
  done <<< "$all_candidates"

  if [ "$tried" -eq 0 ]; then
    err "候选列表为空，无可用节点"
  else
    err "已回退尝试 $tried 次仍未恢复在线"
  fi
  return 1
}

auto_fastest() {
  with_lock
  load_settings

  if ! cooldown_ok; then
    info "冷却中（${COOLDOWN_SECONDS}s），fastest 不动作"
    return 0
  fi
  if node_refresh_needed; then
    node_list_refresh || true
  fi

  init_ban_file

  local cur
  cur=$(current_node_name)
  [ -n "$cur" ] && info "当前节点：$cur" || info "当前节点：未知（无法从 frpc.toml + 缓存/API 反查）"

  health_check
  local st reason proxy_status proxy_reason
  st=$(read_status)
  reason=$(read_status_reason)
  proxy_status=$(read_proxy_status)
  proxy_reason=$(read_proxy_reason)
  local need_switch=0
  if proxy_requires_switch "$proxy_reason"; then
    need_switch=1
  fi
  if [ "$proxy_status" != "ok" ] && [ "$proxy_status" != "unknown" ]; then
    warn "当前代理状态：$proxy_status（$proxy_reason）"
    if [ -n "$cur" ]; then
      info "fastest 前置检测到代理层降级，先对当前节点执行 reconcile"
      reconcile_current_node
      local reconcile_rc=$?
      health_check
      st=$(read_status)
      reason=$(read_status_reason)
      proxy_status=$(read_proxy_status)
      proxy_reason=$(read_proxy_reason)
      info "reconcile 后状态：$st"
      if [ "$reconcile_rc" -eq 11 ]; then
        warn "当前节点已判定为节点侧不可用，继续选择其他节点"
        need_switch=1
      elif [ "$proxy_status" != "ok" ] && [ "$proxy_status" != "unknown" ]; then
        case "$proxy_reason" in
          server_node_conflict|node_unusable|router_conflict)
            warn "reconcile 后仍是节点侧冲突（$proxy_reason），继续切换到更优节点"
            need_switch=1
            ;;
          *)
            warn "reconcile 后代理状态仍为：$proxy_status（$proxy_reason）"
            return 1
            ;;
        esac
      fi
    else
      return 1
    fi
  fi

  local all_candidates
  all_candidates=$(select_best_node || true)
  [ -n "$all_candidates" ] || { err "无法选出任何候选节点"; return 1; }

  # select_best_node 现在输出多行（按 score 升序），取第一行即最优
  local best_line
  best_line=$(echo "$all_candidates" | head -n 1)

  local score name dbg
  score=$(printf '%s' "$best_line" | cut -f1)
  name=$(printf '%s' "$best_line" | cut -f2)
  dbg=$(printf '%s' "$best_line" | cut -f5)

  # 防御性校验
  if ! [[ "$score" =~ ^[0-9]+$ ]]; then
    err "select_best_node 输出格式异常（非数字 score）: [$best_line]"
    return 1
  fi
  [ -n "$name" ] || { err "选出节点名称为空"; return 1; }

  info "最优节点：$name (score=$score, $dbg)"

  if [ -n "$cur" ] && [ "$cur" = "$name" ]; then
    if [ "$need_switch" -ne 1 ]; then
      info "最优节点与当前一致，不切换"
      return 0
    fi
    warn "最优节点就是当前冲突节点，先长冷却再重新选点：$name"
    ban_node_long "$name" "$proxy_reason" "health=$st proxy=$proxy_reason"
    all_candidates=$(select_best_node || true)
    [ -n "$all_candidates" ] || { err "当前冲突节点已冷却，但没有其他可用节点"; return 1; }
    best_line=$(echo "$all_candidates" | head -n 1)
    score=$(printf '%s' "$best_line" | cut -f1)
    name=$(printf '%s' "$best_line" | cut -f2)
    dbg=$(printf '%s' "$best_line" | cut -f5)
    [ -n "$name" ] || { err "重新选出的节点名称为空"; return 1; }
    info "冲突节点冷却后重新选出的最优节点：$name (score=$score, $dbg)"
  fi

  info "fastest：执行切换到 $name"
  apply_switch_to_node "$name"
  local switch_rc=$?
  if [ "$switch_rc" -eq 0 ]; then
    mark_switched
    health_check
    st=$(read_status)
    reason=$(read_status_reason)
    proxy_status=$(read_proxy_status)
    proxy_reason=$(read_proxy_reason)
    info "切换后状态：$st"
    if [ "$proxy_status" != "ok" ] && [ "$proxy_status" != "unknown" ]; then
      warn "fastest 切换后代理状态：$proxy_status（$proxy_reason）"
      info "fastest 检测到代理层降级，尝试对当前节点 reconcile"
      reconcile_current_node || true
      health_check
      st=$(read_status)
      reason=$(read_status_reason)
      proxy_status=$(read_proxy_status)
      proxy_reason=$(read_proxy_reason)
      info "reconcile 后状态：$st"
      if [ "$proxy_status" != "ok" ] && [ "$proxy_status" != "unknown" ]; then
        warn "reconcile 后代理状态仍为：$proxy_status（$proxy_reason）"
        return 1
      fi
    fi
    if status_requires_manual_fix "$reason"; then
      warn "fastest 切换后检测到配置类问题（reason=$reason），停止并请手动检查"
      return 1
    fi
    [ "$st" = "online" ] && success "fastest 切换完成：$name" || warn "fastest 切换后未在线，请观察日志"
    return 0
  fi
  handle_sync_result_for_node "$name"
  local handle_rc=$?
  if [ "$handle_rc" -eq 11 ]; then
    err "fastest 检测到节点级不可用，已执行长冷却：$name"
    return 1
  fi
  err "fastest 切换执行失败：$name"
  return 1
}

manual_switch() {
  with_lock
  load_settings
  # CA User Scripts 的参数输入不支持“一个参数里包含空格”。
  # 因此：
  # - 若传了参数：把剩余参数拼成节点名称
  # - 若没传参数：从 $LOG_DIR/manual_node.txt 读取节点名称（推荐方式，最稳）
  local node_name="$*"
  if [ -z "$node_name" ]; then
    local f="$LOG_DIR/manual_node.txt"
    if [ -f "$f" ]; then
      node_name=$(head -n 1 "$f" 2>/dev/null | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')
    fi
  fi
  [ -n "$node_name" ] || {
    err "manual 缺少节点名称：请传参，或在 $LOG_DIR/manual_node.txt 第一行写入节点名称";
    return 2;
  }
  info "手动切换到节点：$node_name"
  apply_switch_to_node "$node_name"
}

delete_tunnel_v2_test() {
  with_lock
  load_settings
  require_cmd curl || { err "缺少 curl"; return 1; }
  require_cmd jq || { err "缺少 jq"; return 1; }

  local tunnel_id="$1"
  if [ -z "$tunnel_id" ]; then
    err "缺少 tunnelid，用法：$0 delete_v2_test 273588"
    return 2
  fi

  local access_token
  access_token=$(get_access_token) || {
    err "无法获取 access_token"
    return 1
  }

  local usertoken
  if ! ensure_userinfo; then
    warn "userinfo 不存在或同步失败，usertoken 方案将跳过"
    usertoken=""
  else
    usertoken=$(json_get "$USERINFO_FILE" '.data.usertoken // empty')
  fi

  info "开始测试 V2 删除隧道接口 => tunnelid=$tunnel_id"
  info "测试 1：GET + Bearer access_token"
  curl -sS -X GET "https://cf-v2.uapis.cn/delete_tunnel?tunnelid=${tunnel_id}" \
    -H "Authorization: Bearer ${access_token}"
  printf '\n'

  info "测试 2：POST + Bearer access_token"
  curl -sS -X POST "https://cf-v2.uapis.cn/delete_tunnel?tunnelid=${tunnel_id}" \
    -H "Authorization: Bearer ${access_token}"
  printf '\n'

  info "测试 3：POST + query token=access_token"
  curl -sS -X POST "https://cf-v2.uapis.cn/delete_tunnel?token=${access_token}&tunnelid=${tunnel_id}"
  printf '\n'

  if [ -n "$usertoken" ]; then
    info "测试 4：POST + query token=usertoken"
    curl -sS -X POST "https://cf-v2.uapis.cn/delete_tunnel?token=${usertoken}&tunnelid=${tunnel_id}"
    printf '\n'
  fi

  success "V2 删除隧道测试完成"
  return 0
}

usage() {
  cat <<EOF
用法：
  $0 health                      # 健康检查
  $0 failover                   # 故障自愈
  $0 fastest                    # 主动选最优
  $0 reconcile                  # 仅对当前节点做一次同步修复
  $0 manual "节点名称"          # 手动切换节点
  $0 userinfo                   # 同步用户详情
  $0 nodes                      # 刷新节点列表
  $0 delete_v2_test 273588      # 测试 V2 删除隧道接口
  $0 oauth_refresh               # 刷新 OAuth2 token
  $0 oauth_reauth               # 重新进行 V1 OAuth2 授权

推荐给 Unraid CA User Scripts 的一行命令：
  bash "$LOG_DIR/chmlfrp.sh" health
  bash "$LOG_DIR/chmlfrp.sh" failover
  bash "$LOG_DIR/chmlfrp.sh" fastest
  bash "$LOG_DIR/chmlfrp.sh" reconcile
EOF
}

###############################################################################
# QZhua OAuth2 Token 管理
###############################################################################

# 检查是否启用 OAuth2
is_oauth2_enabled() {
  if ! json_ok "$USERDATA_FILE"; then
    return 1
  fi
  local enabled
  enabled=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.enabled // false')
  [ "$enabled" = "true" ]
}

# 获取 token 过期时间戳
get_token_expires_at() {
  json_get "$USERDATA_FILE" '.chmlfrp.oauth2.token_expires_at // 0'
}

# 检查 token 是否过期（预留 60 秒缓冲）
is_token_expired() {
  local expires_at now
  expires_at=$(get_token_expires_at)
  now=$(now_ts)
  [ "$expires_at" -gt 0 ] && [ $((now + TOKEN_EXPIRE_BUFFER)) -ge "$expires_at" ]
}

# 更新 OAuth2 token 到配置文件
update_oauth2_token() {
  local access_token="$1"
  local refresh_token="$2"
  local expires_in="$3"
  
  local now expires_at
  now=$(now_ts)
  expires_at=$((now + expires_in))
  
  if json_ok "$USERDATA_FILE"; then
    local tmp_file="${USERDATA_FILE}.tmp"
    jq --arg at "$access_token" \
       --arg rt "$refresh_token" \
       --argjson ea "$expires_at" \
       '.chmlfrp.oauth2.access_token = $at | .chmlfrp.oauth2.refresh_token = $rt | .chmlfrp.oauth2.token_expires_at = $ea' \
       "$USERDATA_FILE" > "$tmp_file" && mv "$tmp_file" "$USERDATA_FILE"
    echo "[INFO] Token 已保存到 $USERDATA_FILE" >&2
  fi
}

# 用 refresh_token 刷新 access_token
refresh_access_token() {
  require_cmd curl || { echo "[ERROR] 缺少 curl" >&2; return 1; }
  
  local refresh_token
  refresh_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.refresh_token // empty')
  
  if [ -z "$refresh_token" ]; then
    echo "[ERROR] 没有 refresh_token，需要重新授权" >&2
    return 1
  fi
  
  local client_id client_secret
  client_id=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.client_id // empty')
  client_secret=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.client_secret // empty')
  
  if [ -z "$client_id" ] || [ -z "$client_secret" ]; then
    echo "[ERROR] 缺少 client_id 或 client_secret" >&2
    return 1
  fi
  
  echo "[INFO] 正在刷新 access_token..." >&2
  
  local resp
  resp=$(curl -sS -X POST "$QZHUA_TOKEN_ENDPOINT" \
    -u "${client_id}:${client_secret}" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "grant_type=refresh_token" \
    -d "refresh_token=${refresh_token}")
  
  if echo "$resp" | jq -e '.access_token' >/dev/null 2>&1; then
    local new_access_token new_refresh_token expires_in
    new_access_token=$(echo "$resp" | jq -r '.access_token')
    new_refresh_token=$(echo "$resp" | jq -r '.refresh_token // "'"$refresh_token"'"')
    expires_in=$(echo "$resp" | jq -r '.expires_in // 3600')
    
    update_oauth2_token "$new_access_token" "$new_refresh_token" "$expires_in"
    echo "[SUCCESS] access_token 刷新成功" >&2
    return 0
  else
    echo "[ERROR] refresh_token 刷新失败: $(echo "$resp" | jq -r '.error // "未知错误"')" >&2
    return 1
  fi
}

# 设备码授权流程（首次授权或 token 全部失效）
device_code_auth() {
  require_cmd curl || { err "缺少 curl"; return 1; }
  
  local client_id client_secret
  client_id=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.client_id // empty')
  client_secret=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.client_secret // empty')
  
  if [ -z "$client_id" ] || [ -z "$client_secret" ]; then
    err "缺少 client_id 或 client_secret"
    return 1
  fi
  
  info "获取设备码..."
  
  local resp
  resp=$(curl -sS -X POST "$QZHUA_DEVICE_CODE_ENDPOINT" \
    -u "${client_id}:${client_secret}" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "scope=${QZHUA_SCOPE}")
  
  if ! echo "$resp" | jq empty >/dev/null 2>&1; then
    err "获取设备码失败: $resp"
    return 1
  fi
  
  local device_code user_code verification_uri_complete
  device_code=$(echo "$resp" | jq -r '.device_code')
  user_code=$(echo "$resp" | jq -r '.user_code')
  verification_uri_complete=$(echo "$resp" | jq -r '.verification_uri_complete')
  
  if [ -z "$device_code" ] || [ "$device_code" = "null" ]; then
    err "获取 device_code 失败"
    return 1
  fi
  
  echo ""
  echo "========================================"
  echo "请在 5 分钟内完成以下授权操作："
  echo "========================================"
  echo ""
  echo "1. 在浏览器打开以下链接："
  echo "   $verification_uri_complete"
  echo ""
  echo "2. 或访问: https://account-api.qzhua.net/oauth-device-verify"
  echo ""
  echo "3. 输入用户代码: $user_code"
  echo ""
  echo "4. 使用 QZhua 账号登录并授权"
  echo ""
  echo "========================================"
  echo ""
  
  info "轮询获取 access_token（请在浏览器完成授权后）..."
  
  local interval=5
  local max_attempts=60
  
  for ((i=0; i<max_attempts; i++)); do
    sleep "$interval"
    
    resp=$(curl -sS -X POST "$QZHUA_TOKEN_ENDPOINT" \
      -u "${client_id}:${client_secret}" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "grant_type=urn:ietf:params:oauth:grant-type:device_code" \
      -d "device_code=${device_code}")
    
    if echo "$resp" | jq -e '.access_token' >/dev/null 2>&1; then
      local access_token refresh_token expires_in
      access_token=$(echo "$resp" | jq -r '.access_token')
      refresh_token=$(echo "$resp" | jq -r '.refresh_token // ""')
      expires_in=$(echo "$resp" | jq -r '.expires_in // 3600')
      
      update_oauth2_token "$access_token" "$refresh_token" "$expires_in"
      success "授权成功！access_token 已保存"
      return 0
    fi
    
    local error
    error=$(echo "$resp" | jq -r '.error // "unknown"')
    
    if [ "$error" = "authorization_pending" ]; then
      info "等待授权... ($((i+1))/${max_attempts})"
      continue
    elif [ "$error" = "slow_down" ]; then
      info "请求过于频繁，等待..."
      continue
    else
      err "授权失败: $error"
      return 1
    fi
  done
  
  err "授权超时，请重试"
  return 1
}

# 获取有效的 access_token（主入口）
get_access_token() {
  if ! is_oauth2_enabled; then
    echo "[ERROR] OAuth2 未启用，请在 userdata.txt 中启用 oauth2" >&2
    return 1
  fi
  
  local access_token
  access_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.access_token // empty')
  
  if [ -z "$access_token" ] || [ "$access_token" = "null" ]; then
    echo "[INFO] 未找到 access_token，尝试刷新..." >&2
    if refresh_access_token; then
      access_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.access_token // empty')
    else
      echo "[ERROR] ========================================" >&2
      echo "[ERROR] OAuth2 Token 完全失效，需要重新授权！" >&2
      echo "[ERROR] 请运行以下命令完成授权：" >&2
      echo "[ERROR]   ./chmlfrp.sh oauth_reauth" >&2
      echo "[ERROR] 或：" >&2
  echo "[ERROR]   bash \"/mnt/user/Hdd_Disk_Share/脚本日志/chmlfrp/chmlfrp.sh\" oauth_reauth" >&2
      echo "[ERROR] ========================================" >&2
      return 1
    fi
  elif is_token_expired; then
    echo "[INFO] access_token 已过期，尝试刷新..." >&2
    if ! refresh_access_token; then
      echo "[ERROR] ========================================" >&2
      echo "[ERROR] OAuth2 Token 刷新失败，需要重新授权！" >&2
      echo "[ERROR] 请运行以下命令完成授权：" >&2
      echo "[ERROR]   ./chmlfrp.sh oauth_reauth" >&2
      echo "[ERROR] ========================================" >&2
      return 1
    else
      access_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.access_token // empty')
    fi
  fi
  
  echo "$access_token"
}

# 新增命令：刷新 token
oauth_token_refresh() {
  with_lock
  load_settings
  
  if ! is_oauth2_enabled; then
    err "OAuth2 未启用，请先在 userdata.txt 中配置 oauth2"
    return 1
  fi
  
  if ! refresh_access_token; then
    err "Token 刷新失败"
    return 1
  fi
  
  success "Token 刷新成功"
  return 0
}

# 新增命令：重新授权
oauth_reauth() {
  with_lock
  load_settings
  
  if ! is_oauth2_enabled; then
    err "OAuth2 未启用，请先在 userdata.txt 中配置 oauth2"
    return 1
  fi
  
  if ! device_code_auth; then
    err "授权失败"
    return 1
  fi
  
  success "授权成功"
  return 0
}

main() {
  load_settings
  case "${1:-}" in
    health)        health_check ;;
    failover)      auto_failover ;;
    fastest)       auto_fastest ;;
    reconcile)     with_lock; load_settings; reconcile_current_node ;;
    manual)        shift; manual_switch "$*" ;;
    userinfo)      with_lock; load_settings; userinfo_sync ;;
    nodes)          with_lock; load_settings; node_list_refresh ;;
    delete_v2_test) shift; delete_tunnel_v2_test "$1" ;;
    oauth_refresh)  oauth_token_refresh ;;
    oauth_reauth)   oauth_reauth ;;
    -h|--help|help|"") usage ;;
    *) err "未知命令: $1"; usage; exit 2 ;;
  esac
}

main "$@"
