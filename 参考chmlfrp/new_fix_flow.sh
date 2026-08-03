#!/bin/bash
set -o pipefail

BASE_DIR="$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)"
SHARED_LIB="$BASE_DIR/chmlfrp_shared.sh"
[ -f "$SHARED_LIB" ] || SHARED_LIB="/mnt/user/Hdd_Disk_Share/脚本日志/chmlfrp/chmlfrp_shared.sh"
[ -f "$SHARED_LIB" ] || { echo "[ERROR] 缺少共享库 chmlfrp_shared.sh" >&2; exit 1; }
# shellcheck disable=SC1090
. "$SHARED_LIB"

##############################################################################
#
#  ChmlFrp + Cloudflare 新修复流程脚本
#
#  文件建议命名：new_fix_flow.sh
#
#  逻辑分层：
#    (A) 基础层：配置 / 参数 / 日志
#    (B) 工具与配置读取
#    (C) 节点选优（nodeinfo + 在线状态 + ping）
#    (D) ChmlFrp / Cloudflare API 封装
#    (E) 数据准备：固定隧道 + 隧道列表 + DNS 列表
#    (F) 操作层：删 / 建 隧道 & DNS
#    (G) frpc 配置与 Docker 容器
#    (H) 主流程 main()
#
#  重要说明：
#    1）固定隧道文件 chmlfrp固定隧道.txt 必须是“新版规范字段”，不再做老字段兼容：
#        name / tunnel_local_ip / tunnel_local_port / tunnel_type /
#        tunnel_remote_port / dns_domain_cname / dns_proxied / ...
#    2）TCP / UDP 隧道必须在固定文件里显式设置 tunnel_remote_port，
#       本脚本不再自动随机分配远程端口。
#    3）dry-run 模式：
#       运行时加参数 --dry-run
#       只打印将执行的操作，不真正调用 API / 删除 / 创建 / 重启 Docker。
#    4）force-run 模式：
#       运行时加参数 --force-run
#       跳过 mtime + 在线测试文件 + frpc 日志错误这些前置检查，强制执行修复流程。
#
##############################################################################

##############################################################################
# (A) 基础层：全局常量 & 参数 & 日志
##############################################################################

# ------------------ A.1 全局常量 & 文件路径（可被参数覆盖） ------------------

LOG_DIR="${LOG_DIR:-/mnt/user/Hdd_Disk_Share/脚本日志/chmlfrp}"
LOG_FILE=""

MAX_LOG_LINES=1000
HOST_IP="$(hostname -I | awk '{print $1}')"  # 本机 IP（仅日志用）

USERDATA_FILE="$LOG_DIR/userdata.txt"

SETTINGS_FILE="$LOG_DIR/settings.env"

FRPC_DOCKER_NAME="frpc"
FRPC_DOCKER_IMAGE="snowdreamtech/frpc"
FRPC_CONFIG_PATH="${FRPC_CONFIG_PATH:-/mnt/user/appdata/frpc/frpc.toml}"

PRIMARY_DOMAIN="335356119.xyz"

# 节点 / 豁免 / 时间戳缓存
NODE_FILE=""
NODE_ALL_FILE=""
EXEMPT_NODE_FILE=""
FIXED_TS_FILE=""

SYNC_ISSUES_FILE=""
TUNNEL_NAME_OVERRIDES_FILE=""

# 固定豁免隧道/域名（名称匹配）
EXEMPT_ITEMS=("zerotier" "test")

# 关键数据文件
FIXED_TUNNEL_FILE=""
TUNNEL_USERDATA_FILE=""
TEMP_FILE_FIXED=""

TUNNEL_LIST_RAW=""
TUNNEL_LIST_FORMATTED=""
DNS_RAW_FILE=""
DNS_FORMATTED_FILE=""

# 在线测试文件（外部脚本产出，JSON，包含 .status）
ONLINE_TEST_FILE=""

# 统计量
TUNNELS_CREATED=0
TUNNELS_DELETED=0
TUNNELS_CHANGE=0

# 模式开关（可用参数修改）
DNS_ONLY=false           # --dns-only
CLEAN_INVALID_ONLY=false # --clean-invalid
FORCE_RUN=false          # --force-run：跳过前置检查，强制执行修复流程
DRY_RUN=false            # --dry-run：只打印操作，不真正执行
PREFER_FRPC_NODE=false   # --prefer-frpc-node：优先尝试当前 frpc 节点（需 nodeinfo 在线 + ping 通过）
SOURCE_PRECHECK_OK=false

# 指定节点（由 --node 传入；用于与 chmlfrp.sh 对齐）
FORCE_NODE_NAME=""

# 节点选择结果
BEST_NODE_NAME=""
BEST_NODE_IP=""
CURRENT_NODE_IP=""

##############################################################################
# (A.2) 路径初始化 & 日志
##############################################################################

init_paths() {
  chmlfrp_init_shared_layout "$LOG_DIR"
  mkdir -p "$LOG_DIR"

  LOG_FILE="$LOG_DIR/日志-新修复流程.log"

    # 固定隧道清单（优先使用 fixed_tunnels.txt；兼容旧文件名 chmlfrp固定隧道.txt）
  local fixed1="$LOG_DIR/fixed_tunnels.txt"
  local fixed2="$LOG_DIR/chmlfrp固定隧道.txt"
  if [ -f "$fixed1" ]; then
    FIXED_TUNNEL_FILE="$fixed1"
  else
    FIXED_TUNNEL_FILE="$fixed2"
  fi
  TUNNEL_USERDATA_FILE="$LOG_DIR/chmlfrp-userinfo.txt"
  TEMP_FILE_FIXED="$LOG_DIR/chmlfrp-fixed-tunnels-normalized.txt"

  TUNNEL_LIST_RAW="$LOG_DIR/chmlfrp-tunnels-raw.txt"
  TUNNEL_LIST_FORMATTED="$LOG_DIR/chmlfrp-tunnels-normalized.txt"

  DNS_RAW_FILE="$LOG_DIR/cloudflare-dns-raw.txt"
  DNS_FORMATTED_FILE="$LOG_DIR/cloudflare-dns-normalized.txt"

  NODE_FILE="$LOG_DIR/chmlfrp-nodes-filtered.txt"
  NODE_ALL_FILE="$LOG_DIR/chmlfrp-nodes-all.txt"
  EXEMPT_NODE_FILE="$LOG_DIR/chmlfrp豁免列表.txt"
  FIXED_TS_FILE="$LOG_DIR/chmlfrp-source-snapshot.txt"
  ONLINE_TEST_FILE="$LOG_DIR/chmlfrp-health-status.txt"
  SYNC_ISSUES_FILE="$LOG_DIR/chmlfrp-sync-issues.json"
  TUNNEL_NAME_OVERRIDES_FILE="$LOG_DIR/chmlfrp-tunnel-name-overrides.json"
  NODE_ISSUES_FILE="$LOG_DIR/chmlfrp-node-issues.txt"
  SYNC_RESULT_FILE="$LOG_DIR/chmlfrp-sync-result.txt"
}

log_rotate_and_print() {
  local level="$1"
  local msg="$2"
  local ts
  ts="$(date '+%Y-%m-%d %H:%M:%S')"
  local line="[$ts][$level] $msg"

  if [ -z "$LOG_FILE" ]; then
    echo "$line" >&2
    return
  fi

  mkdir -p "$(dirname "$LOG_FILE")" 2>/dev/null
  # 写到 stderr（不被 $(...) 捕获） AND 追加到日志文件
  echo "$line" >&2
  echo "$line" >> "$LOG_FILE"

  local line_count
  line_count=$(wc -l < "$LOG_FILE" 2>/dev/null || echo 0)
  if [ "$line_count" -gt "$MAX_LOG_LINES" ]; then
    if tail -n "$MAX_LOG_LINES" "$LOG_FILE" > "${LOG_FILE}.tmp" 2>/dev/null; then
      mv "${LOG_FILE}.tmp" "$LOG_FILE"
      echo "[$(date '+%Y-%m-%d %H:%M:%S')][INFO] 日志已截断，仅保留最后 $MAX_LOG_LINES 行" >&2
    fi
  fi
}

info()    { log_rotate_and_print "INFO"    "$1"; }
warning() { log_rotate_and_print "WARNING" "$1"; }
error()   { log_rotate_and_print "ERROR"   "$1"; }
success() { log_rotate_and_print "SUCCESS" "$1"; }
debug()   { log_rotate_and_print "DEBUG"   "$1"; }

mask_token() {
  local token="$1"
  if [ -z "$token" ] || [ "$token" = "null" ]; then
    echo "<empty>"
    return 0
  fi
  printf '%s***' "${token:0:4}"
}

api_log_request() {
  local tag="$1" method="$2" url="$3" auth_desc="$4"
  if [ -n "$auth_desc" ]; then
    info "[API][$tag][REQUEST] method=$method url=$url auth=$auth_desc"
  else
    info "[API][$tag][REQUEST] method=$method url=$url"
  fi
}

api_log_response() {
  local tag="$1" resp="$2"
  if [ -z "$resp" ]; then
    warning "[API][$tag][RESPONSE] empty"
    return 0
  fi
  if echo "$resp" | jq empty >/dev/null 2>&1; then
    local code state msg
    code=$(echo "$resp" | jq -r '.code // .success // "n/a"')
    state=$(echo "$resp" | jq -r '.state // "n/a"')
    msg=$(echo "$resp" | jq -r '.msg // .error // ""')
    info "[API][$tag][RESPONSE] code=$code state=$state msg=$msg"
  else
    local preview
    preview=$(printf '%s' "$resp" | tr '\r\n' ' ' | cut -c 1-300)
    warning "[API][$tag][RESPONSE] non_json preview=$preview"
  fi
}

write_json_file() {
  local file="$1"
  local content="$2"
  local tmp="${file}.tmp"
  printf '%s\n' "$content" > "$tmp" && mv "$tmp" "$file"
}

##############################################################################
# (A.3) 参数解析
##############################################################################

usage() {
  cat <<EOF
用法: $0 [options]

选项说明：
  --dns-only          仅同步 DNS，不删除/创建隧道，不更新 frpc 容器
  --clean-invalid     仅清理不在固定配置中的隧道和 DNS，不新建，不更新 frpc
  --prefer-frpc-node  优先尝试沿用当前 frpc 配置对应节点（必须 nodeinfo 显示在线且 ping 正常）
  --force-run         跳过 mtime/在线测试/frpc 日志等前置检查，强制执行修复流程
  --node NAME         指定目标节点名称（例："成都电信"），用于与 chmlfrp.sh 的 failover 参数对齐
  --userdata PATH     指定 userdata.json 路径（默认: $USERDATA_FILE）
  --log-dir PATH      指定日志和中间文件目录（默认: $LOG_DIR）
  --frpc-config PATH  指定 frpc.toml 路径（默认: $FRPC_CONFIG_PATH）
  --dry-run           仅打印将执行的操作，不真正调用 API/删除/创建/重启 Docker
  -h, --help          显示本帮助

常见用法示例：
  1）标准修复流程：
     $0

  2）强制执行（跳过前置检查）：
     $0 --force-run

  3）仅同步 DNS：
     $0 --dns-only

  4）仅清理无效资源：
     $0 --clean-invalid

  5）dry-run（只看日志不动真资源）：
     $0 --dry-run
EOF
}

parse_args() {
  local USERDATA_OVERRIDDEN=false
  local LOG_DIR_OVERRIDDEN=false
  while [ $# -gt 0 ]; do
    case "$1" in
      --dns-only)          DNS_ONLY=true ;;
      --clean-invalid)     CLEAN_INVALID_ONLY=true ;;
      --prefer-frpc-node)  PREFER_FRPC_NODE=true ;;  # 新增：优先使用当前 frpc 节点
      --force-run)         FORCE_RUN=true ;;
      --node)             shift; FORCE_NODE_NAME="$1" ;;
      --userdata)          shift; USERDATA_FILE="$1"; USERDATA_OVERRIDDEN=true ;;
      --log-dir)           shift; LOG_DIR="$1"; LOG_DIR_OVERRIDDEN=true ;;
      --frpc-config)       shift; FRPC_CONFIG_PATH="$1" ;;
      --dry-run)           DRY_RUN=true ;;
      -h|--help)           usage; exit 0 ;;
      *)                   warning "未知参数: $1 (已忽略)" ;;
    esac
    shift
  done

  # 若未显式传入 --userdata，则随 --log-dir 变化而调整默认路径
  if ! $USERDATA_OVERRIDDEN; then
    USERDATA_FILE="$LOG_DIR/userdata.txt"
  fi

  if $LOG_DIR_OVERRIDDEN; then
    SETTINGS_FILE="$LOG_DIR/settings.env"
  fi

  # 冲突模式处理：dns-only 优先，其次 clean-invalid
  if $DNS_ONLY && $CLEAN_INVALID_ONLY; then
    warning "检测到同时指定 --dns-only 与 --clean-invalid，优先采用 --dns-only 模式"
    CLEAN_INVALID_ONLY=false
  fi
}

load_settings() {
  chmlfrp_load_settings
}

##############################################################################
# (B) 工具 & 配置读取
##############################################################################

# ------------------ B.0 文件哈希（用于检测内容变更，比 mtime 更可靠） ------------------
file_hash() {
  local file="$1"
  if [ ! -f "$file" ]; then
    echo ""
    return 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
    return 0
  fi
  if command -v md5sum >/dev/null 2>&1; then
    md5sum "$file" | awk '{print $1}'
    return 0
  fi
  # fallback：尽量返回一个稳定值（不一定强）
  wc -c "$file" | awk '{print $1}'
  return 0
}

check_json_file() {
  local file="$1"
  info "检查 JSON 文件: $file"
  if [ ! -f "$file" ]; then
    error "文件不存在: $file"
    return 1
  fi
  if ! jq empty "$file" >/dev/null 2>&1; then
    error "文件 JSON 格式无效: $file"
    return 1
  fi
  success "JSON 文件校验通过: $file"
  return 0
}

is_exempt_name() {
  local nm="${1,,}"
  for e in "${EXEMPT_ITEMS[@]}"; do
    if [[ "$nm" == "${e,,}" ]]; then
      return 0
    fi
  done
  return 1
}

init_json_file() {
  local file="$1"
  local default_json="$2"
  if [ ! -f "$file" ] || ! jq empty "$file" >/dev/null 2>&1; then
    write_json_file "$file" "$default_json"
  fi
}

ensure_sync_issues_file() {
  init_json_file "$SYNC_ISSUES_FILE" '{"records":[]}'
}

record_sync_issue() {
  local name="$1"
  local issue="$2"
  local detail="$3"

  if ! command -v jq >/dev/null 2>&1; then
    warning "无法记录同步问题（缺少 jq）：name=$name issue=$issue detail=$detail"
    return 0
  fi

  ensure_sync_issues_file
  local tmp="${SYNC_ISSUES_FILE}.tmp"
  jq --arg n "$name" --arg i "$issue" --arg d "$detail" --argjson ts "$(date +%s)" '
    .records += [{name:$n, issue:$i, detail:$d, ts:$ts}]' \
    "$SYNC_ISSUES_FILE" > "$tmp" && mv "$tmp" "$SYNC_ISSUES_FILE"
}

write_sync_result() {
  local status="$1"
  local classification="$2"
  local message="$3"
  local node_name="${4:-$BEST_NODE_NAME}"
  local detail="${5:-}"
  chmlfrp_write_sync_result "$status" "$classification" "$message" "$node_name" "$detail"
}

count_recent_proxy_conflicts() {
  local logs="$1"
  printf '%s\n' "$logs" | grep -Eci 'router config conflict|route config conflict|proxy \[.*\] already exists'
}

ensure_tunnel_name_overrides_file() {
  init_json_file "$TUNNEL_NAME_OVERRIDES_FILE" '{"overrides":{}}'
}

get_tunnel_name_override() {
  local original="$1"
  ensure_tunnel_name_overrides_file
  jq -r --arg n "$original" '.overrides[$n] // empty' "$TUNNEL_NAME_OVERRIDES_FILE" 2>/dev/null
}

set_tunnel_name_override() {
  local original="$1"
  local overridden="$2"

  ensure_tunnel_name_overrides_file
  local tmp="${TUNNEL_NAME_OVERRIDES_FILE}.tmp"
  jq --arg n "$original" --arg v "$overridden" '.overrides[$n] = $v' \
    "$TUNNEL_NAME_OVERRIDES_FILE" > "$tmp" && mv "$tmp" "$TUNNEL_NAME_OVERRIDES_FILE"
}

make_unique_tunnel_name() {
  local base="$1"
  local ts
  ts=$(date '+%Y%m%d%H%M%S')
  printf '%s__ts%s_p%s' "$base" "$ts" "$$"
}

snapshot_tunnel_line_list() {
  local file="$1"
  if [ ! -f "$file" ]; then
    return 1
  fi

  jq -r '
    (.tunnels // [])[] |
    [
      (.runtime_name // .name // ""),
      (.tunnel_local_ip // ""),
      (.tunnel_local_port // ""),
      (.tunnel_type // ""),
      (.tunnel_remote_port // ""),
      (.dns_domain_cname // .name // ""),
      ((.dns_proxied // false) | tostring)
    ] | @tsv' "$file" 2>/dev/null | while IFS=$'\t' read -r name ip port ttype rport cname proxied; do
      [ -n "$name" ] || continue
      local norm_name norm_cname
      norm_name="$(sanitize_tunnel_name "$name")"
      norm_cname="$(sanitize_tunnel_name "$cname")"
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$norm_name" "$ip" "$port" "$ttype" "$rport" "$norm_cname" "$proxied"
    done | LC_ALL=C sort
}

tunnel_snapshot_fingerprint() {
  local file="$1"
  local tmp="${file}.fingerprint.$$"
  if ! snapshot_tunnel_line_list "$file" > "$tmp"; then
    rm -f "$tmp" 2>/dev/null || true
    return 1
  fi
  local hash
  hash=$(file_hash "$tmp") || hash=""
  rm -f "$tmp" 2>/dev/null || true
  printf '%s' "$hash"
}

tunnel_id_by_name() {
  local target="$1"
  [ -f "$TUNNEL_LIST_FORMATTED" ] || return 1

  while IFS=$'\t' read -r name tid; do
    [ -n "$name" ] || continue
    if [ "$(sanitize_tunnel_name "$name")" = "$(sanitize_tunnel_name "$target")" ]; then
      printf '%s' "$tid"
      return 0
    fi
  done < <(jq -r '.tunnels[] | [.name, (.id // empty)] | @tsv' "$TUNNEL_LIST_FORMATTED" 2>/dev/null)

  return 1
}

sanitize_tunnel_name() {
  local raw="$1"
  raw="$(printf '%s' "$raw" | tr '[:upper:]' '[:lower:]')"
  raw="$(printf '%s' "$raw" | sed -E 's/[^a-z0-9._-]+/-/g; s/-+/-/g; s/^-+//; s/-+$//')"
  raw="$(printf '%s' "$raw" | sed -E 's/[._-]*[0-9]+$//; s/[._-]+$//')"
  [ -n "$raw" ] || raw="tunnel"
  printf '%s' "$raw"
}

build_tunnel_runtime_name() {
  local base_name="$1"
  local safe_name
  safe_name="$(sanitize_tunnel_name "$base_name")"

  # 运行名只做字符归一，并自动剔除尾部数字后缀；简洁且便于维护。
  printf '%s' "$safe_name"
}

tunnel_name_exists_in_remote_state() {
  local tunnel_name="$1"
  [ -f "$TUNNEL_LIST_FORMATTED" ] || return 1

  local normalized_candidate
  normalized_candidate="$(sanitize_tunnel_name "$tunnel_name")"

  while IFS= read -r remote_name; do
    [ -n "$remote_name" ] || continue
    if [ "$(sanitize_tunnel_name "$remote_name")" = "$normalized_candidate" ]; then
      return 0
    fi
  done < <(jq -r '.tunnels[]?.name // empty' "$TUNNEL_LIST_FORMATTED")

  return 1
}

is_fixed_tunnel_name() {
  local candidate="$1"
  [ -f "$TEMP_FILE_FIXED" ] || return 1

  while IFS= read -r item; do
    local base_name local_port runtime_name
    base_name="$(echo "$item" | jq -r '.name // ""')"
    local_port="$(echo "$item" | jq -r '.tunnel_local_port // ""')"
    runtime_name="$(echo "$item" | jq -r '.runtime_name // empty')"

    [ -z "$base_name" ] && continue

    if [ "$candidate" = "$base_name" ]; then
      return 0
    fi

    if [ -z "$runtime_name" ]; then
      runtime_name="$(build_tunnel_runtime_name "$base_name" "$local_port")"
    fi

    if [ "$candidate" = "$runtime_name" ]; then
      return 0
    fi
  done < <(jq -c '.tunnels[]' "$TEMP_FILE_FIXED")

  return 1
}

# ------------------ B.1 读取 userdata 配置 ------------------

CHMLFRP_USER=""
CHMLFRP_TOKEN=""
CF_EMAIL=""
CF_API_TOKEN=""
CF_ZONE_ID=""

# QZhua OAuth2 配置
OAUTH2_ENABLED="false"
OAUTH2_CLIENT_ID=""
OAUTH2_CLIENT_SECRET=""
OAUTH2_ACCESS_TOKEN=""
OAUTH2_REFRESH_TOKEN=""
OAUTH2_TOKEN_EXPIRES_AT=""

# Token 缓存（避免在一次运行中重复刷新）
CACHED_ACCESS_TOKEN=""
CACHED_TOKEN_FETCHED=false

read_configs_from_userdata() {
  info "开始读取配置文件 => $USERDATA_FILE"

  if [ ! -f "$USERDATA_FILE" ]; then
    error "配置文件不存在: $USERDATA_FILE"
    return 1
  fi
  if ! jq empty "$USERDATA_FILE" >/dev/null 2>&1; then
    error "配置文件 JSON 格式无效: $USERDATA_FILE"
    return 1
  fi

  CHMLFRP_USER=$(jq -r '.chmlfrp.username // empty' "$USERDATA_FILE")
  CHMLFRP_TOKEN=$(jq -r '.chmlfrp.token // empty' "$USERDATA_FILE")
  CF_EMAIL=$(jq -r '.cloudflare.email // empty' "$USERDATA_FILE")
  CF_API_TOKEN=$(jq -r '.cloudflare.api_token // empty' "$USERDATA_FILE")
  CF_ZONE_ID=$(jq -r '.cloudflare.zone_id // empty' "$USERDATA_FILE")

  # 读取 OAuth2 配置
  OAUTH2_ENABLED=$(jq -r '.chmlfrp.oauth2.enabled // false' "$USERDATA_FILE")
  OAUTH2_CLIENT_ID=$(jq -r '.chmlfrp.oauth2.client_id // empty' "$USERDATA_FILE")
  OAUTH2_CLIENT_SECRET=$(jq -r '.chmlfrp.oauth2.client_secret // empty' "$USERDATA_FILE")
  OAUTH2_ACCESS_TOKEN=$(jq -r '.chmlfrp.oauth2.access_token // empty' "$USERDATA_FILE")
  OAUTH2_REFRESH_TOKEN=$(jq -r '.chmlfrp.oauth2.refresh_token // empty' "$USERDATA_FILE")
  OAUTH2_TOKEN_EXPIRES_AT=$(jq -r '.chmlfrp.oauth2.token_expires_at // 0' "$USERDATA_FILE")

  if [ "$OAUTH2_ENABLED" = "true" ]; then
    info "检测到 OAuth2 认证已启用"
    if [ -z "$OAUTH2_CLIENT_ID" ] || [ -z "$OAUTH2_CLIENT_SECRET" ]; then
      error "OAuth2 已启用但缺少 client_id 或 client_secret"
      return 1
    fi
  else
    # 旧模式：需要直接 token
    if [ -z "$CHMLFRP_USER" ] || [ -z "$CHMLFRP_TOKEN" ]; then
      error "ChmlFrp 配置不完整（需要 chmlfrp.username 和 chmlfrp.token）"
      return 1
    fi
  fi

  if [ -z "$CF_EMAIL" ] || [ -z "$CF_API_TOKEN" ] || [ -z "$CF_ZONE_ID" ]; then
    error "Cloudflare 配置不完整"
    return 1
  fi

  local ctoken_head="${CHMLFRP_TOKEN:0:5}"
  local cf_token_head="${CF_API_TOKEN:0:5}"
  success "读取配置成功："
  info "  - ChmlFrp: username=$CHMLFRP_USER, token前5位=$ctoken_head..."
  info "  - Cloudflare: email=$CF_EMAIL, token前5位=$cf_token_head..., zone_id=$CF_ZONE_ID"
  if [ "$OAUTH2_ENABLED" = "true" ]; then
    info "  - OAuth2: enabled=true"
  fi
  return 0
}

# ------------------ B.2 读取当前 frpc 节点 IP ------------------

read_current_node_ip() {
  local toml_file="$FRPC_CONFIG_PATH"
  info "读取 FRPC 配置 => $toml_file"

  if [ ! -f "$toml_file" ]; then
    warning "FRPC 配置文件不存在 => $toml_file"
    return 1
  fi

  local ip
  ip=$(grep -E '^\s*server_addr\s*=' "$toml_file" | sed -E 's/^\s*server_addr\s*=\s*"?([^"]*)"?/\1/')

  if [ -z "$ip" ]; then
    warning "FRPC 配置中未找到 server_addr => $toml_file"
    return 1
  fi

  CURRENT_NODE_IP="$ip"
  info "读取到当前节点 IP => $CURRENT_NODE_IP"
  return 0
}

# ------------------ B.3 frpc 日志错误检测 ------------------

check_frpc_logs_for_config_mismatch() {
  info "检查 frpc 容器日志中是否存在配置错误（客户端代理参数错误 / router config conflict）..."

  if ! docker ps -a --format '{{.Names}}' | grep -wq "$FRPC_DOCKER_NAME"; then
    warning "frpc 容器 [$FRPC_DOCKER_NAME] 不存在，无法检查日志"
    return 1
  fi

  local last_logs
  last_logs=$(docker logs --tail 80 "$FRPC_DOCKER_NAME" 2>/dev/null)

  if [ -z "$last_logs" ]; then
    info "frpc 日志为空，无此错误"
    return 1
  fi

  local count_mismatch
  count_mismatch=$(echo "$last_logs" | grep -c "start error: 客户端代理参数错误")

  if [ "$count_mismatch" -gt 0 ]; then
    warning "发现 $count_mismatch 次 [客户端代理参数错误] 日志，疑似配置不匹配！"
    record_sync_issue "frpc" "config_mismatch" "tail_80_logs_contains_client_proxy_parameter_error"
    return 0
  else
    local count_router_conflict
    count_router_conflict=$(echo "$last_logs" | grep -cE "router config conflict|route config conflict")
    if [ "$count_router_conflict" -gt 0 ]; then
      warning "发现 $count_router_conflict 次 [router config conflict] 日志，疑似路由配置冲突！"
      record_sync_issue "frpc" "router_conflict" "tail_80_logs_contains_router_config_conflict"
      return 0
    fi

    info "未检测到 [客户端代理参数错误] 或 [router config conflict] 日志"
    return 1
  fi
}

# ------------------ B.4 源文件时间戳 & 在线测试前置检查 ------------------

TS_FIXED_NOW=0
TS_EXEMPT_NOW=0
HASH_FIXED_NOW=""
HASH_EXEMPT_NOW=""

check_source_ts() {
  local cfg="$FIXED_TUNNEL_FILE"
  local exm="$EXEMPT_NODE_FILE"
  local cache="$FIXED_TS_FILE"
  local test_file="$ONLINE_TEST_FILE"

  # 1) 检查关键源文件是否存在（固定隧道必需；豁免文件可选）
  if [ ! -f "$cfg" ]; then
    error "关键源文件缺失: $cfg"
    exit 1
  fi
  if [ ! -f "$exm" ]; then
    warning "豁免列表文件不存在，将按空文件处理: $exm"
    touch "$exm" 2>/dev/null || true
  fi

  # 2) 读取当前 mtime + 内容 hash
  TS_FIXED_NOW=$(stat -c %Y "$cfg")
  TS_EXEMPT_NOW=$(stat -c %Y "$exm")
  HASH_FIXED_NOW=$(file_hash "$cfg")
  HASH_EXEMPT_NOW=$(file_hash "$exm")

  info "固定隧道文件: $cfg"
  info "  - mtime: $TS_FIXED_NOW"
  info "  - hash : ${HASH_FIXED_NOW:0:12}..."
  info "豁免列表文件: $exm"
  info "  - mtime: $TS_EXEMPT_NOW"
  info "  - hash : ${HASH_EXEMPT_NOW:0:12}..."

  # 3) 在线测试状态
  local test_status="unknown"
  if [[ -f "$test_file" ]] && jq empty "$test_file" >/dev/null 2>&1; then
    test_status=$(jq -r '.status // "unknown"' "$test_file")
  else
    warning "未找到在线测试文件或格式非法 => $test_file → 视为 status=offline"
    test_status="offline"
  fi
  info "在线测试状态: $test_status"

  # 在线测试文件过旧则视为 offline（避免 stale=online 导致误退出）
  if [ "$test_status" = "online" ] && [ -f "$test_file" ]; then
    local now_ts test_mtime age
    now_ts=$(date +%s)
    test_mtime=$(stat -c %Y "$test_file" 2>/dev/null || echo 0)
    age=$((now_ts - test_mtime))
    if [ "$age" -gt 900 ]; then
      warning "在线测试文件过旧（${age}s），将视为 offline 触发修复"
      test_status="offline"
    fi
  fi

  # 4) 检查 frpc 日志错误
  local frpc_error=0
  if check_frpc_logs_for_config_mismatch; then
    frpc_error=1
    warning "frpc 日志检测到配置类错误（客户端代理参数错误 / router config conflict），将继续执行修复流程"
  fi

  # 5) 读取上次缓存的 mtime
  local ts_fixed_last=""
  local ts_exempt_last=""
  local hash_fixed_last=""
  local hash_exempt_last=""
  if [[ -f "$cache" ]] && jq empty "$cache" >/dev/null 2>&1; then
    ts_fixed_last=$(jq -r '.fixed_tunnel_mtime // ""' "$cache")
    ts_exempt_last=$(jq -r '.exempt_file_mtime  // ""' "$cache")
    hash_fixed_last=$(jq -r '.fixed_tunnel_hash // ""' "$cache")
    hash_exempt_last=$(jq -r '.exempt_file_hash  // ""' "$cache")
    info "缓存中的 fixed_tunnel_mtime: ${ts_fixed_last:-无}"
    info "缓存中的 fixed_tunnel_hash : ${hash_fixed_last:0:12}..."
    info "缓存中的 exempt_file_mtime : ${ts_exempt_last:-无}"
    info "缓存中的 exempt_file_hash  : ${hash_exempt_last:0:12}..."
  else
    info "未找到有效的 mtime/hash 缓存文件: $cache (视为首次运行或缓存失效)"
  fi

  SOURCE_PRECHECK_OK=false

  # 6) 判断是否满足“预检通过但仍需核对远端快照”的条件
  if [[ "$frpc_error" -eq 0 \
        && -n "$ts_fixed_last" \
        && -n "$hash_fixed_last" \
        && -n "$ts_exempt_last" \
        && -n "$hash_exempt_last" \
        && "$TS_FIXED_NOW" == "$ts_fixed_last" \
        && "$HASH_FIXED_NOW" == "$hash_fixed_last" \
        && "$TS_EXEMPT_NOW" == "$ts_exempt_last" \
        && "$HASH_EXEMPT_NOW" == "$hash_exempt_last" \
        && "$test_status" == "online" ]]; then
    SOURCE_PRECHECK_OK=true
    success "固定隧道/豁免文件 mtime+hash 均未变化，在线测试正常且 frpc 日志无错误 ➜ 预检通过，稍后将核对远端隧道快照。"
  else
    info "本次预检未通过，后续将继续执行修复/重建流程，触发原因如下："

    if [[ -z "$ts_fixed_last" || -z "$hash_fixed_last" || -z "$ts_exempt_last" || -z "$hash_exempt_last" ]]; then
      info " - 未找到完整的历史 mtime/hash 缓存记录 (首次运行或缓存损坏)"
    else
      if [[ "$TS_FIXED_NOW" != "$ts_fixed_last" ]]; then
        info " - 固定隧道文件 mtime 发生变化: $ts_fixed_last -> $TS_FIXED_NOW"
      else
        info " - 固定隧道文件 mtime 未变化"
      fi

      if [[ "$HASH_FIXED_NOW" != "$hash_fixed_last" ]]; then
        info " - 固定隧道文件内容发生变化(hash): ${hash_fixed_last:0:12}... -> ${HASH_FIXED_NOW:0:12}..."
      else
        info " - 固定隧道文件内容未变化(hash)"
      fi

      if [[ "$TS_EXEMPT_NOW" != "$ts_exempt_last" ]]; then
        info " - 豁免列表文件 mtime 发生变化: $ts_exempt_last -> $TS_EXEMPT_NOW"
      else
        info " - 豁免列表文件 mtime 未变化"
      fi

      if [[ "$HASH_EXEMPT_NOW" != "$hash_exempt_last" ]]; then
        info " - 豁免列表文件内容发生变化(hash): ${hash_exempt_last:0:12}... -> ${HASH_EXEMPT_NOW:0:12}..."
      else
        info " - 豁免列表文件内容未变化(hash)"
      fi
    fi

    if [[ "$test_status" != "online" ]]; then
      info " - 在线测试状态非 online: $test_status"
    else
      info " - 在线测试状态为 online"
    fi

    if [[ "$frpc_error" -eq 1 ]]; then
      info " - 检测到 frpc 日志中存在 [客户端代理参数错误] 或 [router config conflict]"
    else
      info " - frpc 日志正常，无配置错误信息"
    fi
  fi
}

write_source_ts_cache() {
  local cfg="$FIXED_TUNNEL_FILE"
  local exm="$EXEMPT_NODE_FILE"
  local ts_fixed=0
  local ts_exempt=0
  local hash_fixed=""
  local hash_exempt=""

  if [ -f "$cfg" ]; then
    ts_fixed=$(stat -c %Y "$cfg" 2>/dev/null || echo 0)
    hash_fixed=$(file_hash "$cfg")
  fi
  if [ -f "$exm" ]; then
    ts_exempt=$(stat -c %Y "$exm" 2>/dev/null || echo 0)
    hash_exempt=$(file_hash "$exm")
  fi

  jq -n \
    --argjson t1 "$ts_fixed" --argjson t2 "$ts_exempt" \
    --arg h1 "$hash_fixed" --arg h2 "$hash_exempt" \
    '{fixed_tunnel_mtime:$t1, exempt_file_mtime:$t2, fixed_tunnel_hash:$h1, exempt_file_hash:$h2}' > "$FIXED_TS_FILE"

  success "已更新 mtime/hash 缓存文件 => $FIXED_TS_FILE (fixed_mtime=$ts_fixed fixed_hash=${hash_fixed:0:12}..., exempt_mtime=$ts_exempt exempt_hash=${hash_exempt:0:12}...)"
}

# ------------------ B.5 读取用户详情 ID ------------------

CHMLFRP_USERID=""

sync_userdetail_with_access_token() {
  local token
  token=$(get_access_token) || {
    warning "无法获取 access_token，无法同步用户详情"
    return 1
  }

  local url resp code
  url="https://cf-v2.uapis.cn/login?access_token=${token}"
  api_log_request "login" "GET" "https://cf-v2.uapis.cn/login?access_token=$(mask_token "$token")" "oauth2_access_token"
  resp=$(curl -sS -L --connect-timeout 5 --max-time 15 "$url" || true)
  api_log_response "login" "$resp"

  if [ -z "$resp" ] || ! echo "$resp" | jq empty >/dev/null 2>&1; then
    warning "用户详情接口返回空或非 JSON"
    return 1
  fi

  code=$(echo "$resp" | jq -r '.code // 0')
  if [ "$code" != "200" ]; then
    warning "同步用户详情失败 => code=$code, msg=$(echo "$resp" | jq -r '.msg // ""')"
    return 1
  fi

  write_json_file "$TUNNEL_USERDATA_FILE" "$(echo "$resp" | jq '.')"
  success "用户详情已写入 => $TUNNEL_USERDATA_FILE"
  return 0
}

read_userdetail_id() {
  info "读取 ChmlFrp 用户详情 => $TUNNEL_USERDATA_FILE"

  if [ ! -f "$TUNNEL_USERDATA_FILE" ]; then
    warning "用户详情文件不存在 => 尝试自动同步"
    sync_userdetail_with_access_token || return 1
  fi
  if ! jq empty "$TUNNEL_USERDATA_FILE" >/dev/null 2>&1; then
    warning "用户详情 JSON 不合法 => 尝试重新同步"
    sync_userdetail_with_access_token || return 1
  fi

  local tmpid
  tmpid=$(jq -r '.data.id // 0' "$TUNNEL_USERDATA_FILE")
  if [ "$tmpid" = "0" ] || [ -z "$tmpid" ]; then
    warning "无法获取 userdetail id => $tmpid"
    return 1
  fi

  CHMLFRP_USERID="$tmpid"
  success "解析到用户 ID => CHMLFRP_USERID=$CHMLFRP_USERID"
  return 0
}

# ------------------ B.6 URL 编码 ------------------

urlencode() {
  local string="$1"
  local encoded
  encoded=$(curl -s -o /dev/null -w '%{url_effective}' \
    --get --data-urlencode "name=$string" 'http://dummy' | sed 's/.*name=//g')
  if [ -z "$encoded" ]; then
    warning "URL 编码失败，返回原始字符串"
    echo "$string"
    return 1
  fi
  echo "$encoded"
  return 0
}

##############################################################################
# (B.7) QZhua OAuth2 Token 管理
##############################################################################

# 检查 token 是否过期（预留 60 秒缓冲）
is_token_expired() {
  local now
  now=$(date +%s)
  [ "$OAUTH2_TOKEN_EXPIRES_AT" -gt 0 ] && [ $((now + 60)) -ge "$OAUTH2_TOKEN_EXPIRES_AT" ]
}

# 更新 OAuth2 token 到配置文件
update_oauth2_token() {
  local access_token="$1"
  local refresh_token="$2"
  local expires_in="$3"
  
  local now expires_at
  now=$(date +%s)
  expires_at=$((now + expires_in))
  
  if [ -f "$USERDATA_FILE" ]; then
    local tmp_file="${USERDATA_FILE}.tmp"
    jq --arg at "$access_token" \
       --arg rt "$refresh_token" \
       --argjson ea "$expires_at" \
       '.chmlfrp.oauth2.access_token = $at | .chmlfrp.oauth2.refresh_token = $rt | .chmlfrp.oauth2.token_expires_at = $ea' \
       "$USERDATA_FILE" > "$tmp_file" && mv "$tmp_file" "$USERDATA_FILE"
    
    OAUTH2_ACCESS_TOKEN="$access_token"
    OAUTH2_REFRESH_TOKEN="$refresh_token"
    OAUTH2_TOKEN_EXPIRES_AT="$expires_at"
    
    echo "[INFO] Token 已保存到 $USERDATA_FILE" >&2
  fi
}

# 刷新 access_token
refresh_access_token() {
  if [ -z "$OAUTH2_REFRESH_TOKEN" ] || [ -z "$OAUTH2_CLIENT_ID" ] || [ -z "$OAUTH2_CLIENT_SECRET" ]; then
    echo "[ERROR] OAuth2 刷新参数不完整" >&2
    return 1
  fi

  echo "[INFO] 正在刷新 access_token..." >&2

  local resp
  resp=$(curl -sS -X POST "https://account-api.qzhua.net/oauth2/token" \
    -u "${OAUTH2_CLIENT_ID}:${OAUTH2_CLIENT_SECRET}" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "grant_type=refresh_token" \
    -d "refresh_token=${OAUTH2_REFRESH_TOKEN}")

  if echo "$resp" | jq -e '.access_token' >/dev/null 2>&1; then
    local new_access_token new_refresh_token expires_in
    new_access_token=$(echo "$resp" | jq -r '.access_token')
    new_refresh_token=$(echo "$resp" | jq -r '.refresh_token // "'"$OAUTH2_REFRESH_TOKEN"'"')
    expires_in=$(echo "$resp" | jq -r '.expires_in // 3600')

    update_oauth2_token "$new_access_token" "$new_refresh_token" "$expires_in"

    # 更新缓存
    CACHED_ACCESS_TOKEN="$new_access_token"
    CACHED_TOKEN_FETCHED=true

    echo "[SUCCESS] access_token 刷新成功" >&2
    return 0
  else
    local err_msg
    err_msg=$(echo "$resp" | jq -r '.error // "未知错误"')
    echo "[ERROR] 刷新失败: $err_msg" >&2
    if [ "$err_msg" = "invalid_grant" ]; then
      echo "[ERROR] refresh_token 已失效，需要重新授权" >&2
    fi
    return 1
  fi
}

# 获取有效的 access_token（主入口）
get_access_token() {
  if [ "$OAUTH2_ENABLED" != "true" ]; then
    # 兼容旧模式
    echo "$CHMLFRP_TOKEN"
    return 0
  fi
  
  # 检查是否有 token
  if [ -z "$OAUTH2_ACCESS_TOKEN" ] || [ "$OAUTH2_ACCESS_TOKEN" = "null" ]; then
    echo "[INFO] 未找到 access_token，尝试刷新..." >&2
    if ! refresh_access_token; then
      echo "[ERROR] 无法获取有效的 access_token" >&2
echo "[ERROR] 提示：请运行 'chmlfrp.sh oauth_reauth' 重新获取授权（V1）" >&2
      return 1
    fi
  elif is_token_expired; then
    echo "[INFO] access_token 已过期，尝试刷新..." >&2
    if ! refresh_access_token; then
      echo "[ERROR] 无法刷新 access_token，Token 可能已彻底失效" >&2
echo "[ERROR] 提示：请运行 'chmlfrp.sh oauth_reauth' 重新获取授权（V1）" >&2
      return 1
    fi
  fi

  # 使用缓存避免重复获取
  if [ -n "$CACHED_ACCESS_TOKEN" ] && [ "$CACHED_TOKEN_FETCHED" = "true" ]; then
    echo "$CACHED_ACCESS_TOKEN"
    return 0
  fi

  CACHED_ACCESS_TOKEN="$OAUTH2_ACCESS_TOKEN"
  CACHED_TOKEN_FETCHED=true
  echo "$OAUTH2_ACCESS_TOKEN"
}

##############################################################################
# (C) 节点选优（nodeinfo + 在线状态 + ping）
##############################################################################
# 节点测速选优：通过 nodeinfo 实时获取在线状态 + 实际 IP，然后再 ping 选 RTT 最优
select_best_node() {
  info "开始执行节点测速（实时 nodeinfo + ping 选优）..."

  local PING_ATTEMPTS="${PING_ATTEMPTS:-5}"
  local PING_TIMEOUT="${PING_TIMEOUT:-2}"
  local MIN_SUCCESS="${MIN_SUCCESS:-3}"
  local SLEEP_BETWEEN="${SLEEP_BETWEEN:-0.2}"

  # 1) 读取 node 列表（只要求有 节点名称/节点本地IPv4）
  if [ ! -f "$NODE_FILE" ]; then
    error "节点文件不存在 => $NODE_FILE"
    return 1
  fi
  if ! jq empty "$NODE_FILE" >/dev/null 2>&1; then
    error "节点文件 JSON 格式不合法 => $NODE_FILE"
    return 1
  fi

  local total
  total=$(jq '.nodes | length' "$NODE_FILE")
  if [ "$total" -eq 0 ]; then
    error "节点文件中 nodes 为空 => 无可用节点"
    return 1
  fi

  local -a candidates=()
  local i

  for ((i = 0; i < total; i++)); do
    local name ip_from_file
    name=$(jq -r ".nodes[$i][\"节点名称\"] // \"\"" "$NODE_FILE")
    ip_from_file=$(jq -r ".nodes[$i][\"节点本地IPv4\"] // \"\"" "$NODE_FILE")

    # 名称是必须的，IP 没有也先记录下来，后面可以从 nodeinfo 里拿 realIp
    if [[ -z "$name" ]]; then
      continue
    fi

    # 2) 调用 nodeinfo 实时获取 state / realIp
    local name_encoded url resp token
    name_encoded=$(urlencode "$name")
    token=$(get_access_token) || {
      warning "无法获取 access_token，跳过该节点 => name=$name"
      continue
    }
    url="https://cf-v2.uapis.cn/nodeinfo?token=${token}&node=${name_encoded}"

    info "请求 nodeinfo => name=$name, url=$url"
    resp=$(curl -sS -L --connect-timeout 3 --max-time 5 "$url")

    if [ -z "$resp" ] || ! echo "$resp" | jq empty >/dev/null 2>&1; then
      warning "nodeinfo 请求失败或返回非 JSON，跳过该节点 => name=$name"
      continue
    fi

    local code state real_ip domain_ip
    code=$(echo "$resp"      | jq -r '.code // 0')
    state=$(echo "$resp"     | jq -r '.data.state   // "unknown"')
    real_ip=$(echo "$resp"   | jq -r '.data.realIp  // ""')
    domain_ip=$(echo "$resp" | jq -r '.data.ip      // ""')

    if [ "$code" -ne 200 ]; then
      warning "nodeinfo 返回 code=$code，跳过该节点 => name=$name"
      continue
    fi

    # 3) 只对 state=online 的节点测速
    if [[ "$state" != "online" ]]; then
      info "节点状态非在线，跳过测速 => name=$name, state=$state"
      continue
    fi

    # 4) 选择 ping 目标：优先 realIp，其次本地配置的 节点本地IPv4，再不行就用域名 ip
    local ping_target="$real_ip"
    if [[ -z "$ping_target" || "$ping_target" == "null" ]]; then
      ping_target="$ip_from_file"
    fi
    if [[ -z "$ping_target" || "$ping_target" == "null" ]]; then
      ping_target="$domain_ip"
    fi
    if [[ -z "$ping_target" || "$ping_target" == "null" ]]; then
      warning "节点 [$name] 无可用 IP（realIp / 节点本地IPv4 / 域名都空），跳过"
      continue
    fi

    info "节点在线，通过 nodeinfo 初筛 => name=$name, state=$state, ping_target=$ping_target"
    info "开始 ping 测速 => name=$name, ip=$ping_target"

    # 5) ping 统计 RTT
    local success_cnt=0
    local total_ms=0
    local j

    for ((j = 1; j <= PING_ATTEMPTS; j++)); do
      local rtt
      rtt=$(ping -c 1 -W "$PING_TIMEOUT" "$ping_target" 2>/dev/null \
            | awk -F'time=' '/time=/{print $2}' | awk '{print int($1)}')
      if [[ -n "$rtt" ]]; then
        success_cnt=$((success_cnt + 1))
        total_ms=$((total_ms + rtt))
      fi
      sleep "$SLEEP_BETWEEN"
    done

    if (( success_cnt < MIN_SUCCESS )); then
      warning "节点 ping 不稳定，丢弃 => name=$name, ip=$ping_target, 成功 $success_cnt/$PING_ATTEMPTS"
      continue
    fi

    local avg_ms=$(( total_ms / success_cnt ))
    info "节点测速结果 => name=$name, ip=$ping_target, 成功 $success_cnt/$PING_ATTEMPTS, 平均 RTT=${avg_ms}ms"

    # 用于 DNS 的 IP：优先 realIp，其次 ping_target
    local ip_for_dns="$real_ip"
    if [[ -z "$ip_for_dns" || "$ip_for_dns" == "null" ]]; then
      ip_for_dns="$ping_target"
    fi

    # 记录候选：格式 "avg_ms<TAB>name<TAB>ping_target<TAB>ip_for_dns"
    candidates+=("${avg_ms}"$'\t'"${name}"$'\t'"${ping_target}"$'\t'"${ip_for_dns}")
  done

  # 6) 没有任何在线 + ping 通过的节点
  if (( ${#candidates[@]} == 0 )); then
    error "nodeinfo 中没有在线且 ping 正常的节点 => 无法选优"
    return 1
  fi

  # 7) 按 RTT 升序排序，选出最优节点
  local sorted
  sorted=$(printf '%s\n' "${candidates[@]}" | sort -n -t $'\t' -k1,1)

  local best_line best_rtt best_name best_ip_ping best_ip_dns
  best_line=$(printf '%s\n' "$sorted" | head -n 1)

  best_rtt=$(printf '%s' "$best_line" | cut -f1)
  best_name=$(printf '%s' "$best_line" | cut -f2)
  best_ip_ping=$(printf '%s' "$best_line" | cut -f3)
  best_ip_dns=$(printf '%s' "$best_line" | cut -f4)

  BEST_NODE_NAME="$best_name"
  BEST_NODE_IP="$best_ip_dns"

  success "节点选优完成（nodeinfo 在线 + ping 最优）=> name=$BEST_NODE_NAME, ip=$BEST_NODE_IP, RTT=${best_rtt}ms"
  return 0
}

# 指定节点：根据节点名称做 nodeinfo 在线校验，并解析出用于 DNS 的 IP
select_node_by_name() {
  local name="$1"
  if [ -z "$name" ]; then
    return 1
  fi

  local lookup_file="$NODE_ALL_FILE"
  if [ ! -f "$lookup_file" ] || ! jq empty "$lookup_file" >/dev/null 2>&1; then
    lookup_file="$NODE_FILE"
  fi
  if [ ! -f "$lookup_file" ] || ! jq empty "$lookup_file" >/dev/null 2>&1; then
    error "节点文件不存在或非法 => $NODE_ALL_FILE / $NODE_FILE"
    return 1
  fi

  local idx
  idx=$(jq -r --arg name "$name" '
    .nodes | to_entries[] | select((.value["节点名称"] // .value.name)==$name) | .key
  ' "$lookup_file" | head -n 1)
  if [ -z "$idx" ] || [ "$idx" = "null" ]; then
    error "在节点文件中找不到指定节点名称 => $name"
    return 1
  fi

  local name_encoded url resp code state real_ip domain_ip ip_from_file ping_target token
  ip_from_file=$(jq -r ".nodes[$idx][\"节点本地IPv4\"] // .nodes[$idx].realIp // .nodes[$idx].ip // \"\"" "$lookup_file")

  name_encoded=$(urlencode "$name")
  token=$(get_access_token) || {
    error "无法获取 access_token => name=$name"
    return 1
  }
  url="https://cf-v2.uapis.cn/nodeinfo?token=${token}&node=${name_encoded}"
  info "指定节点模式：请求 nodeinfo 校验 => name=$name, url=$url"
  resp=$(curl -sS -L --connect-timeout 3 --max-time 5 "$url")

  if [ -z "$resp" ] || ! echo "$resp" | jq empty >/dev/null 2>&1; then
    error "nodeinfo 请求失败或返回非 JSON => name=$name"
    return 1
  fi

  code=$(echo "$resp"  | jq -r '.code // 0')
  state=$(echo "$resp" | jq -r '.data.state // "unknown"')
  real_ip=$(echo "$resp" | jq -r '.data.realIp // ""')
  domain_ip=$(echo "$resp" | jq -r '.data.ip // ""')

  if [ "$code" -ne 200 ]; then
    error "nodeinfo 返回 code=$code => name=$name"
    return 1
  fi
  if [ "$state" != "online" ]; then
    error "指定节点不在线 => name=$name, state=$state"
    return 1
  fi

  ping_target="$real_ip"
  if [ -z "$ping_target" ] || [ "$ping_target" = "null" ]; then ping_target="$ip_from_file"; fi
  if [ -z "$ping_target" ] || [ "$ping_target" = "null" ]; then ping_target="$domain_ip"; fi
  if [ -z "$ping_target" ] || [ "$ping_target" = "null" ]; then
    error "指定节点无可用 IP（realIp/节点本地IPv4/域名ip均为空）=> name=$name"
    return 1
  fi

  BEST_NODE_NAME="$name"
  # DNS 优先用 realIp，否则用 ping_target
  BEST_NODE_IP="$real_ip"
  if [ -z "$BEST_NODE_IP" ] || [ "$BEST_NODE_IP" = "null" ]; then BEST_NODE_IP="$ping_target"; fi

  return 0
}


# 尝试优先使用当前 frpc 节点：
# 1）CURRENT_NODE_IP 必须存在
# 2）在 nodeinfo 中能找到对应记录
# 3）该记录在线（online/在线/up）
# 4）ping 测试通过（成功次数 >= MIN_SUCCESS）
prefer_node_from_frpc() {
  if ! $PREFER_FRPC_NODE; then
    return 1
  fi

  if [ -z "$CURRENT_NODE_IP" ]; then
    warning "prefer_node_from_frpc: 当前 frpc 未读取到 server_addr IP，无法优先使用"
    return 1
  fi

  local lookup_file="$NODE_ALL_FILE"
  if [ ! -f "$lookup_file" ] || ! jq empty "$lookup_file" >/dev/null 2>&1; then
    lookup_file="$NODE_FILE"
  fi
  if [ ! -f "$lookup_file" ] || ! jq empty "$lookup_file" >/dev/null 2>&1; then
    warning "prefer_node_from_frpc: 节点文件不存在或非法 => $NODE_ALL_FILE / $NODE_FILE"
    return 1
  fi

  # 1) 在 node 列表中按 IP 匹配节点
  local idx
  idx=$(jq -r --arg ip "$CURRENT_NODE_IP" '
    .nodes
    | to_entries[]
    | select((.value."节点本地IPv4" // .value.realIp // .value.ip // "") == $ip)
    | .key
  ' "$lookup_file" | head -n 1)

  if [ -z "$idx" ] || [ "$idx" = "null" ]; then
    warning "prefer_node_from_frpc: 在 node 列表中找不到 IP=$CURRENT_NODE_IP 对应的节点，放弃优先使用"
    return 1
  fi

  local name
  name=$(jq -r ".nodes[$idx][\"节点名称\"] // .nodes[$idx].name // \"\"" "$lookup_file")
  if [ -z "$name" ]; then
    warning "prefer_node_from_frpc: 匹配到的节点名称为空，放弃优先使用"
    return 1
  fi

  # 2) 调用 nodeinfo 检查状态 + 真实 IP
  local name_encoded url resp token
  name_encoded=$(urlencode "$name")
  token=$(get_access_token) || {
    warning "prefer_node_from_frpc: 无法获取 access_token，放弃优先使用"
    return 1
  }
  url="https://cf-v2.uapis.cn/nodeinfo?token=${token}&node=${name_encoded}"

  info "prefer_node_from_frpc: 调用 nodeinfo 校验当前节点 => name=$name, url=$url"
  resp=$(curl -sS -L --connect-timeout 3 --max-time 5 "$url")

  if [ -z "$resp" ] || ! echo "$resp" | jq empty >/dev/null 2>&1; then
    warning "prefer_node_from_frpc: nodeinfo 请求失败或返回非 JSON，放弃优先使用当前节点"
    return 1
  fi

  local code state real_ip domain_ip
  code=$(echo "$resp"      | jq -r '.code // 0')
  state=$(echo "$resp"     | jq -r '.data.state   // "unknown"')
  real_ip=$(echo "$resp"   | jq -r '.data.realIp  // ""')
  domain_ip=$(echo "$resp" | jq -r '.data.ip      // ""')

  if [ "$code" -ne 200 ]; then
    warning "prefer_node_from_frpc: nodeinfo 返回 code=$code，放弃优先使用当前节点"
    return 1
  fi

  if [[ "$state" != "online" ]]; then
    warning "prefer_node_from_frpc: 节点 [$name] 在 nodeinfo 中状态=$state（非 online），放弃优先使用"
    return 1
  fi

  # 3) 选择用于 ping 的目标
  local ping_target="$real_ip"
  if [[ -z "$ping_target" || "$ping_target" == "null" ]]; then
    ping_target="$CURRENT_NODE_IP"
  fi
  if [[ -z "$ping_target" || "$ping_target" == "null" ]]; then
    ping_target="$domain_ip"
  fi
  if [[ -z "$ping_target" || "$ping_target" == "null" ]]; then
    warning "prefer_node_from_frpc: 当前节点无可用 IP，放弃优先使用 => name=$name"
    return 1
  fi

  info "prefer_node_from_frpc: 节点在线，通过 nodeinfo 初筛 => name=$name, state=$state, ping_target=$ping_target"
  info "prefer_node_from_frpc: 对当前 frpc 节点做 ping 测试..."

  local PING_ATTEMPTS_LOCAL="${PING_ATTEMPTS:-5}"
  local PING_TIMEOUT_LOCAL="${PING_TIMEOUT:-2}"
  local MIN_SUCCESS_LOCAL="${MIN_SUCCESS:-3}"
  local SLEEP_BETWEEN_LOCAL="${SLEEP_BETWEEN:-0.2}"

  local success_cnt=0 total_ms=0 j
  for ((j = 1; j <= PING_ATTEMPTS_LOCAL; j++)); do
    local rtt
    rtt=$(ping -c 1 -W "$PING_TIMEOUT_LOCAL" "$ping_target" 2>/dev/null \
          | awk -F'time=' '/time=/{print $2}' | awk '{print int($1)}')
    if [[ -n "$rtt" ]]; then
      success_cnt=$((success_cnt + 1))
      total_ms=$((total_ms + rtt))
    fi
    sleep "$SLEEP_BETWEEN_LOCAL"
  done

  if (( success_cnt < MIN_SUCCESS_LOCAL )); then
    warning "prefer_node_from_frpc: 当前节点 ping 不稳定，成功 $success_cnt/$PING_ATTEMPTS_LOCAL，放弃优先使用"
    return 1
  fi

  local avg_ms=$(( total_ms / success_cnt ))

  local ip_for_dns="$real_ip"
  if [[ -z "$ip_for_dns" || "$ip_for_dns" == "null" ]]; then
    ip_for_dns="$ping_target"
  fi

  BEST_NODE_NAME="$name"
  BEST_NODE_IP="$ip_for_dns"

  success "prefer_node_from_frpc: 当前 frpc 节点通过 nodeinfo 在线 + ping 校验，将优先使用 => name=$BEST_NODE_NAME, ip=$BEST_NODE_IP, RTT=${avg_ms}ms"
  return 0
}

##############################################################################
# (D) ChmlFrp / Cloudflare API 封装
##############################################################################

# ------------------ D.1 ChmlFrp 隧道 API ------------------

tunnel_api_create() {
  local json_payload="$1"
  local url="https://cf-v2.uapis.cn/create_tunnel"

  if $DRY_RUN; then
    info "[DRY_RUN][API][create_tunnel][REQUEST] method=POST url=$url payload=$json_payload"
    echo '{"code":200,"msg":"DRY_RUN"}'
    return 0
  fi

  api_log_request "create_tunnel" "POST" "$url" "payload.token=$(echo "$json_payload" | jq -r '.token // ""' | cut -c1-4)***"
  local resp
  resp=$(curl -sS -X POST "$url" \
    -H "Content-Type: application/json" \
    --data-raw "$json_payload")
  api_log_response "create_tunnel" "$resp"
  printf '%s' "$resp"
}

tunnel_api_delete() {
  local nodeid="$1"
  
  # 获取有效 token
  local token
  token=$(get_access_token) || {
    error "无法获取 access_token"
    return 1
  }

  local base_url="https://cf-v2.uapis.cn/delete_tunnel"
  local safe_token="$(mask_token "$token")"
  local url="${base_url}?tunnelid=${nodeid}"

  if $DRY_RUN; then
    info "[DRY_RUN][API][delete_tunnel][REQUEST] method=GET url=${base_url}?tunnelid=${nodeid} auth=Bearer ${safe_token}"
    echo '{"code":200,"error":"DRY_RUN"}'
    return 0
  fi

  api_log_request "delete_tunnel" "GET" "${base_url}?tunnelid=${nodeid}" "Bearer ${safe_token}"

  local resp
  resp=$(curl -sS -X GET "$url" -H "Authorization: Bearer ${token}" || echo "")

  if [ -z "$resp" ]; then
    warning "删除隧道空响应"
    echo '{"code":0,"error":"empty_response"}'
    return 1
  fi

  api_log_response "delete_tunnel" "$resp"

  if ! echo "$resp" | jq empty >/dev/null 2>&1; then
    warning "删除隧道接口返回非 JSON，当前无法可靠删除"
    return 1
  fi

  local code msg
  code=$(echo "$resp" | jq -r '.code // 0')
  msg=$(echo "$resp"  | jq -r '.msg // .error // ""')

  if [ "$code" -eq 200 ]; then
    return 0
  fi

  if [ "$code" -eq 400 ] && echo "$msg" | grep -Eq "此隧道不属于你|隧道不存在|找不到隧道"; then
    info "隧道不属于当前用户，视为已清理"
    return 0
  fi

  return 1
}

tunnel_api_fetch_list() {
  local token
  token=$(get_access_token) || {
    error "无法获取 access_token"
    echo '{"code":401,"msg":"无有效token"}'
    return 1
  }
  local url="https://cf-v2.uapis.cn/tunnel"
  api_log_request "tunnel_list" "GET" "$url" "Bearer $(mask_token "$token")"
  local resp
  resp=$(curl -sS -X GET "${url}" -H "Authorization: Bearer ${token}")
  api_log_response "tunnel_list" "$resp"
  printf '%s' "$resp"
}

# ------------------ D.2 Cloudflare DNS API ------------------

dns_api_create() {
  local subdomain="$1"
  local record_ip="$2"
  local proxied="${3:-false}"

  local full_name="$PRIMARY_DOMAIN"
  if [ -n "$subdomain" ]; then
    full_name="$subdomain.$PRIMARY_DOMAIN"
  fi

  local payload
  payload=$(jq -n \
    --arg name "$full_name" \
    --arg ip   "$record_ip" \
    --argjson px "$proxied" '
    {
      "type": "A",
      "name": $name,
      "content": $ip,
      "ttl": 120,
      "proxied": $px
    }')

  local url="https://api.cloudflare.com/client/v4/zones/$CF_ZONE_ID/dns_records"

  if $DRY_RUN; then
    info "[DRY_RUN] 创建 DNS: $payload"
    echo '{"success":true,"msg":"DRY_RUN"}'
    return 0
  fi

  curl -sS -X POST "$url" \
    -H "Authorization: Bearer $CF_API_TOKEN" \
    -H "Content-Type: application/json" \
    --data-raw "$payload"
}

dns_api_delete() {
  local zone_id="$1"
  local cf_token="$2"
  local record_id="$3"
  local url="https://api.cloudflare.com/client/v4/zones/$zone_id/dns_records/$record_id"

  if $DRY_RUN; then
    info "[DRY_RUN] 删除 DNS 记录 id=$record_id"
    echo '{"success":true,"msg":"DRY_RUN"}'
    return 0
  fi

  curl -sS -X DELETE "$url" \
    -H "Authorization: Bearer $cf_token" \
    -H "Content-Type: application/json"
}

dns_api_fetch_list() {
  local zone_id="$1"
  local cf_token="$2"
  local url="https://api.cloudflare.com/client/v4/zones/$zone_id/dns_records"

  curl -sS -X GET "$url" \
    -H "Authorization: Bearer $cf_token" \
    -H "Content-Type: application/json"
}

##############################################################################
# (E) 数据准备：固定隧道 + 隧道列表 + DNS 列表
##############################################################################

# ------------------ E.1 读取固定隧道配置（只认“新版标准字段”） ------------------

tunnel_get_fixed() {
  info "读取固定隧道配置（新版字段）: $FIXED_TUNNEL_FILE"

  [ -f "$TEMP_FILE_FIXED" ] && rm -f "$TEMP_FILE_FIXED"
  write_json_file "$TEMP_FILE_FIXED" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, tunnels:[]}')"

  if ! check_json_file "$FIXED_TUNNEL_FILE"; then
    error "固定隧道配置校验失败 => $FIXED_TUNNEL_FILE"
    return 1
  fi

  local count=0
  while IFS= read -r line; do
    [ -z "$line" ] && continue

    local name ip port ttype rport cname proxied
    name=$(echo "$line"  | jq -r '.name // ""')
    ip=$(echo "$line"    | jq -r '.tunnel_local_ip // "127.0.0.1"')
    port=$(echo "$line"  | jq -r '.tunnel_local_port // "80"')
    ttype=$(echo "$line" | jq -r '.tunnel_type // "http"')
    rport=$(echo "$line" | jq -r '.tunnel_remote_port // ""')
    cname=$(echo "$line" | jq -r '.dns_domain_cname // ""')
    proxied=$(echo "$line" | jq -r '.dns_proxied // "false"')

    if [ -z "$name" ]; then
      warning "固定隧道配置中无 name 字段，跳过该条记录"
      continue
    fi

    # 对于 tcp/udp 类型，要求 tunnel_remote_port 必须在配置中明确给出，不再随机分配
    if [[ "$ttype" == "tcp" || "$ttype" == "udp" ]]; then
      if [ -z "$rport" ] || [ "$rport" = "null" ]; then
        warning "固定隧道 [$name] 类型=$ttype 但未配置 tunnel_remote_port，跳过该条"
        continue
      fi
    fi

    if [ -z "$cname" ] || [ "$cname" = "null" ]; then
      cname="$name"
    fi

    local runtime_name
    runtime_name="$(build_tunnel_runtime_name "$name" "$port")"

    local overridden_name
    overridden_name="$(get_tunnel_name_override "$name")"
    if [ -n "$overridden_name" ]; then
      runtime_name="$overridden_name"
    fi

    if [ "$proxied" != "true" ] && [ "$proxied" != "false" ]; then
      proxied="false"
    fi

    local item
    item=$(jq -n \
      --arg n "$name" \
      --arg ip "$ip" \
      --arg p "$port" \
      --arg tt "$ttype" \
      --arg rp "$rport" \
      --arg cn "$cname" \
      --arg rn "$runtime_name" \
      --argjson px "$proxied" '
      {
        "name": $n,
        "runtime_name": $rn,
        "tunnel_local_ip": $ip,
        "tunnel_local_port": $p,
        "tunnel_type": $tt,
        "tunnel_remote_port": $rp,
        "dns_domain_cname": $cn,
        "dns_proxied": $px
      }')

    jq --argjson x "$item" '.tunnels += [$x] | .total = (.tunnels | length)' "$TEMP_FILE_FIXED" \
      > "${TEMP_FILE_FIXED}.tmp" && mv "${TEMP_FILE_FIXED}.tmp" "$TEMP_FILE_FIXED"
    count=$((count+1))
  done < <(jq -c '.[]' "$FIXED_TUNNEL_FILE")

  success "固定隧道读取完成 => 总数=$count => 标准化输出: $TEMP_FILE_FIXED"
  return 0
}

# ------------------ E.2 获取并格式化隧道列表 ------------------

tunnel_fetch_list_local() {
  info "获取 ChmlFrp 隧道列表 => $TUNNEL_LIST_RAW"
  [ -f "$TUNNEL_LIST_RAW" ] && rm -f "$TUNNEL_LIST_RAW"

  local max_retries=3
  local attempt=0
  local wait_seconds=5

  while [ $attempt -lt $max_retries ]; do
    attempt=$(( attempt + 1 ))
    info "第 ${attempt} 次尝试获取隧道列表..."
    local resp
    resp=$(tunnel_api_fetch_list)

    if [ -z "$resp" ]; then
      warning "隧道列表 API 无响应 => 重试..."
      sleep $wait_seconds
      continue
    fi
    if ! echo "$resp" | jq empty >/dev/null 2>&1; then
      local raw_preview
      raw_preview=$(echo "$resp" | head -c 500)
      warning "隧道列表返回非 JSON => 重试... (响应预览: ${raw_preview})"
      sleep $wait_seconds
      continue
    fi

    local code
    code=$(echo "$resp" | jq -r '.code // "null"')
    if [ "$code" = "200" ]; then
      write_json_file "$TUNNEL_LIST_RAW" "$(jq -n --argjson ts "$(date +%s)" --arg src "https://cf-v2.uapis.cn/tunnel" --argjson payload "$resp" '{refreshed_at:$ts, source:$src, api_response:$payload}')"
      success "隧道列表获取成功 => 写入 $TUNNEL_LIST_RAW"
      return 0
    elif [ "$code" = "401" ]; then
      error "获取隧道列表失败 => code=401 (Token 无效或已过期)"
      error "请运行 'chmlfrp.sh oauth_reauth' 重新获取 Token"
      return 1
    else
      local msg
      msg=$(echo "$resp" | jq -r '.msg // "未知错误"')
      warning "获取隧道列表失败 => code=$code => $msg => 重试..."
      sleep $wait_seconds
    fi
  done

  error "多次尝试获取隧道列表失败，无法确定现有隧道状态"
  write_json_file "$TUNNEL_LIST_RAW" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, source:"https://cf-v2.uapis.cn/tunnel", api_response:{data:[]}}')"
  return 1
}

tunnel_format_list() {
  info "格式化隧道列表 => $TUNNEL_LIST_FORMATTED"

  # 情况 1：原始文件不存在或为空
  if [[ ! -f "$TUNNEL_LIST_RAW" ]] || [[ ! -s "$TUNNEL_LIST_RAW" ]]; then
    info "当前 ChmlFrp 隧道列表为空 (0 条)"
    write_json_file "$TUNNEL_LIST_FORMATTED" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, tunnels:[]}')"
    return 0
  fi

  # 情况 2：JSON 解析失败
  local data
  if ! data=$(jq '.api_response.data // []' "$TUNNEL_LIST_RAW" 2>/dev/null); then
    info "当前 ChmlFrp 隧道列表为空 (0 条)"
    write_json_file "$TUNNEL_LIST_FORMATTED" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, tunnels:[]}')"
    return 0
  fi

  # 情况 3：data 字段为空数组
  if [[ "$data" == "[]" ]]; then
    info "当前 ChmlFrp 隧道列表为空 (0 条)"
    write_json_file "$TUNNEL_LIST_FORMATTED" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, tunnels:[]}')"
    return 0
  fi

  # 真正有数据的情况
  write_json_file "$TUNNEL_LIST_FORMATTED" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, tunnels:[]}')"
  local count=0

  while IFS= read -r item; do
    local name
    name=$(echo "$item" | jq -r '.name // ""')
    [[ -z "$name" ]] && continue

    local new_entry
    new_entry=$(echo "$item" | jq '
      with_entries(
        if .key == "localip" then .key = "tunnel_local_ip"
        elif .key == "nport" then .key = "tunnel_local_port"
        elif .key == "type"  then .key = "tunnel_type"
        elif .key == "dorp"  then .key = "dns_domain_cname"
        elif .key == "ip"    then .key = "DROP_IP"
        else .
        end
      ) | del(.DROP_IP)
    ')

    new_entry=$(echo "$new_entry" | jq --arg dom "$PRIMARY_DOMAIN" '
      if .dns_domain_cname != null then
        .dns_domain_cname |= (
          gsub("\\.$"; "") |
          sub("\\." + $dom + "$"; "")
        )
      else . end
    ')

    jq --argjson r "$new_entry" '.tunnels += [$r] | .total = (.tunnels | length)' "$TUNNEL_LIST_FORMATTED" \
      > "${TUNNEL_LIST_FORMATTED}.tmp" && mv "${TUNNEL_LIST_FORMATTED}.tmp" "$TUNNEL_LIST_FORMATTED"

    count=$((count + 1))
    info "已格式化隧道 => name=$name"
  done < <(echo "$data" | jq -c '.[]')

  success "隧道列表格式化完成 => total=$count => $TUNNEL_LIST_FORMATTED"
  return 0
}


# ------------------ E.3 获取并格式化 DNS 列表 ------------------

dns_fetch_list_local() {
  info "获取 Cloudflare DNS 列表 => $DNS_RAW_FILE"
  [ -f "$DNS_RAW_FILE" ] && rm -f "$DNS_RAW_FILE"

  local max_retries=3
  local attempt=0
  local wait_seconds=5

  while [ $attempt -lt $max_retries ]; do
    attempt=$(( attempt + 1 ))
    info "第 ${attempt} 次尝试获取 DNS 列表..."
    local resp
    resp=$(dns_api_fetch_list "$CF_ZONE_ID" "$CF_API_TOKEN")

    if [ -z "$resp" ]; then
      warning "DNS API 无响应 => 重试..."
      sleep $wait_seconds
      continue
    fi
    if ! echo "$resp" | jq empty >/dev/null 2>&1; then
      warning "DNS API 返回非 JSON => 重试..."
      sleep $wait_seconds
      continue
    fi

    local ok
    ok=$(echo "$resp" | jq -r '.success // "false"')
    if [ "$ok" = "true" ]; then
      write_json_file "$DNS_RAW_FILE" "$(echo "$resp" | jq --argjson ts "$(date +%s)" '{refreshed_at:$ts, records:(.result // [])}')"
      success "DNS 列表获取成功 => 写入 $DNS_RAW_FILE"
      return 0
    else
      local err
      err=$(echo "$resp" | jq -r '.errors[0].message // "未知错误"')
      warning "获取 DNS 列表失败 => $err => 重试..."
      sleep $wait_seconds
    fi
  done

  error "多次尝试获取 DNS 列表失败，视为当前无 DNS 记录"
  write_json_file "$DNS_RAW_FILE" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, records:[]}')"
  return 1
}

dns_format_list() {
  info "格式化 DNS 列表 => $DNS_FORMATTED_FILE"

  # 情况 1：原始文件不存在
  if [[ ! -f "$DNS_RAW_FILE" ]]; then
    info "当前 Cloudflare DNS 列表为空 (0 条)"
    write_json_file "$DNS_FORMATTED_FILE" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, records:[]}')"
    return 0
  fi

  # 情况 2：JSON 解析失败
  if ! jq empty "$DNS_RAW_FILE" >/dev/null 2>&1; then
    info "当前 Cloudflare DNS 列表为空 (0 条)"
    write_json_file "$DNS_FORMATTED_FILE" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, records:[]}')"
    return 0
  fi

  # 情况 3：数组长度为 0
  if jq -e '(.records // []) | length==0' "$DNS_RAW_FILE" >/dev/null 2>&1; then
    info "当前 Cloudflare DNS 列表为空 (0 条)"
    write_json_file "$DNS_FORMATTED_FILE" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, records:[]}')"
    return 0
  fi

  # 真正有数据的情况
  write_json_file "$DNS_FORMATTED_FILE" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, records:[]}')"
  local count=0

  while IFS= read -r line; do
    [[ -z "$line" ]] && continue

    local full short
    full=$(echo "$line" | jq -r '.name // ""')
    short="${full%.$PRIMARY_DOMAIN}"
    short="${short%.}"

    local new_item
    new_item=$(echo "$line" | jq --arg s "$short" '
      . + {
        "dns_domain_cname": $s,
        "dns_content": .content,
        "dns_proxied": .proxied
      }
      | del(.name, .content, .proxied)
    ')

    jq --argjson x "$new_item" '.records += [$x] | .total = (.records | length)' "$DNS_FORMATTED_FILE" \
      > "${DNS_FORMATTED_FILE}.tmp" && mv "${DNS_FORMATTED_FILE}.tmp" "$DNS_FORMATTED_FILE"

    count=$((count + 1))
    info "格式化 DNS => name=$full => dns_domain_cname=$short"
  done < <(jq -c '.records[]' "$DNS_RAW_FILE")

  success "DNS 格式化完成 => total=$count => $DNS_FORMATTED_FILE"
  return 0
}


##############################################################################
# (F) 操作层：隧道 / DNS 删除 & 创建
##############################################################################

# ------------------ F.1 全量删除所有非豁免的隧道 & DNS ------------------

perform_force_delete_all() {
  info "开始删除所有非豁免隧道 / DNS（全量清空模式）"

  # 1) 隧道多轮拉取 + 删除
  local MAX_ROUNDS=5
  local round total_deleted=0

  for ((round=1; round<=MAX_ROUNDS; round++)); do
    info "---- 隧道删除轮次 #$round ----"
    local resp
    resp=$(tunnel_api_fetch_list)

    if [ -z "$resp" ] || ! echo "$resp" | jq -e '.code==200 and (.data|type=="array")' >/dev/null 2>&1; then
      error "第 ${round} 轮获取隧道列表失败，终止隧道删除流程"
      # 获取响应预览用于诊断
      if [ -n "$resp" ]; then
        local preview
        preview=$(echo "$resp" | head -c 200)
        debug "响应预览: ${preview}"
      fi
      return 1  # 标记失败
    fi

    local to_delete=0 deleted_this_round=0

    while IFS= read -r item; do
      local name id
      name=$(echo "$item" | jq -r '.name // ""')
      id=$(echo "$item"   | jq -r '.id   // empty')
      [ -z "$name" ] && continue
      [ -z "$id" ] && continue

      if is_exempt_name "$name"; then
        info "隧道 [$name] 在豁免列表中，保留 (id=$id)"
        continue
      fi

      to_delete=$((to_delete + 1))
      info "删除隧道 [$name] (id=$id)"

      if tunnel_api_delete "$id" >/dev/null; then
        ((TUNNELS_DELETED++))
        ((deleted_this_round++))
        success "删除隧道成功 [$name] (id=$id)"
      else
        warning "删除隧道失败 [$name] (id=$id)"
      fi
    done < <(echo "$resp" | jq -c '.data[]')

    info "轮次 #$round：计划删除 $to_delete 个，实际成功 $deleted_this_round 个"
    total_deleted=$((total_deleted + deleted_this_round))

    if [ "$to_delete" -gt 0 ] && [ "$deleted_this_round" -eq 0 ]; then
      error "第 ${round} 轮：计划删除 $to_delete 个但实际删除 0 个，可能 Token 已失效"
      return 1
    fi

    if [ "$to_delete" -eq 0 ]; then
      info "轮次 #$round 无可删除隧道，提前结束隧道删除循环"
      break
    fi
  done

  # 检查是否真的成功删除了（防止部分失败）
  if [ "$total_deleted" -eq 0 ]; then
    # 再次尝试获取列表确认状态
    local check_resp
    check_resp=$(tunnel_api_fetch_list 2>/dev/null || echo '{"data":[]}')
    local remaining
    remaining=$(echo "$check_resp" | jq '.data | length' 2>/dev/null || echo 0)
    if [ "$remaining" -gt 0 ]; then
      error "仍有 $remaining 条隧道未删除，但删除操作报告成功删除了 0 条"
      error "请重点检查 delete_tunnel 接口是否按面板抓包方式工作"
      return 1
    fi
  fi

  success "隧道删除结束，总成功删除数量：$total_deleted"

  # 2) DNS 删除
  if [ -f "$DNS_FORMATTED_FILE" ]; then
    local dns_deleted=0

    while IFS=$'\t' read -r cname rid; do
      [ -z "$cname" ] && continue
      [ -z "$rid" ] && continue

      if is_exempt_name "$cname"; then
        info "DNS [$cname] 在豁免列表中，保留 (id=$rid)"
        continue
      fi

      # 仅删除“固定清单”内的 DNS，避免误删同 Zone 的其他记录
      local in_fixed
      in_fixed=$(jq -r --arg c "$cname" '(.tunnels // []) | map((.dns_domain_cname // .name) == $c) | any' "$TEMP_FILE_FIXED" 2>/dev/null || echo "false")
      if [ "$in_fixed" != "true" ]; then
        debug "DNS [$cname] 不在固定清单中，跳过删除"
        continue
      fi

      info "删除 DNS [$cname] (id=$rid)"
      local dresp
      dresp=$(dns_api_delete "$CF_ZONE_ID" "$CF_API_TOKEN" "$rid")

      if echo "$dresp" | jq -e '.success == true' >/dev/null 2>&1 \
         || echo "$dresp" | grep -qi "success"; then
        dns_deleted=$((dns_deleted + 1))
        success "删除 DNS 成功 [$cname] (id=$rid)"
      else
        warning "删除 DNS 失败 [$cname] (id=$rid): $dresp"
      fi
    done < <(jq -r '.records[] | [.dns_domain_cname, .id] | @tsv' "$DNS_FORMATTED_FILE")

    success "DNS 删除完成，成功删除数量：$dns_deleted"
  else
    info "DNS_FORMATTED_FILE 不存在，跳过 DNS 删除"
  fi

  success "非豁免隧道 / DNS 全量清理流程结束"
}

# ------------------ F.2 仅删除非豁免 DNS（dns-only 模式用） ------------------

delete_non_exempt_dns() {
  info "仅删除所有非豁免 DNS 记录"

  if [ -f "$DNS_FORMATTED_FILE" ]; then
    while IFS=$'\t' read -r cname rid; do
      [ -z "$cname" ] && continue
      [ -z "$rid" ] && continue

      if is_exempt_name "$cname"; then
        info "DNS [$cname] 在豁免列表中，保留 (id=$rid)"
        continue
      fi

      # 仅删除“固定清单”内的 DNS，避免误删同 Zone 的其他记录
      local in_fixed
      in_fixed=$(jq -r --arg c "$cname" '(.tunnels // []) | map((.dns_domain_cname // .name) == $c) | any' "$TEMP_FILE_FIXED" 2>/dev/null || echo "false")
      if [ "$in_fixed" != "true" ]; then
        debug "DNS [$cname] 不在固定清单中，跳过删除"
        continue
      fi

      info "删除 DNS [$cname] (id=$rid)"
      dns_api_delete "$CF_ZONE_ID" "$CF_API_TOKEN" "$rid" >/dev/null
    done < <(jq -r '.records[] | [.dns_domain_cname, .id] | @tsv' "$DNS_FORMATTED_FILE")
  fi

  success "DNS 删除完成（仅非豁免记录）"
}

# ------------------ F.3 增量清理（不在固定配置中的隧道 / DNS） ------------------

clean_invalid_resources() {
  info "仅清理不在固定配置中的隧道 / DNS"

  # 清理隧道
  if [ -f "$TUNNEL_LIST_FORMATTED" ] && [ -f "$TEMP_FILE_FIXED" ]; then
    while IFS=$'\t' read -r name tid; do
      [ -z "$name" ] && continue
      [ -z "$tid" ] && continue

      if is_exempt_name "$name"; then
        info "隧道 [$name] 在豁免列表，保留"
        continue
      fi

      if ! is_fixed_tunnel_name "$name"; then
        info "删除未在固定配置中的隧道 [$name] (id=$tid)"
        tunnel_api_delete "$tid" >/dev/null
      else
        debug "隧道 [$name] 存在于固定配置，保留"
      fi
    done < <(jq -r '.tunnels[] | [.name, .id] | @tsv' "$TUNNEL_LIST_FORMATTED")
  fi

  # 清理 DNS
  if [ -f "$DNS_FORMATTED_FILE" ] && [ -f "$TEMP_FILE_FIXED" ]; then
    while IFS=$'\t' read -r cname rid; do
      [ -z "$cname" ] && continue
      [ -z "$rid" ] && continue

      if is_exempt_name "$cname"; then
        info "DNS [$cname] 在豁免列表，保留"
        continue
      fi

      local in_fixed
      in_fixed=$(jq -r --arg c "$cname" '
        (.tunnels // []) | map((.dns_domain_cname // .name) == $c) | any
      ' "$TEMP_FILE_FIXED")
      if [ "$in_fixed" != "true" ]; then
        info "删除未在固定配置中的 DNS [$cname] (id=$rid)"
        dns_api_delete "$CF_ZONE_ID" "$CF_API_TOKEN" "$rid" >/dev/null
      else
        debug "DNS [$cname] 存在于固定配置，保留"
      fi
    done < <(jq -r '.records[] | [.dns_domain_cname, .id] | @tsv' "$DNS_FORMATTED_FILE")
  fi

  success "增量清理无效隧道 / DNS 完成"
}

# ------------------ F.4 从固定配置创建 DNS ------------------

dns_create_from_current_state() {
  info "根据固定配置创建 DNS 记录"

  if [ ! -f "$TEMP_FILE_FIXED" ]; then
    error "当前状态文件不存在: $TEMP_FILE_FIXED"
    return 1
  fi

  local external_ip="$1"
  [ -z "$external_ip" ] && external_ip="$CURRENT_NODE_IP"

  local count=0
  while IFS= read -r item; do
    local cname proxied record_ip
    cname=$(echo "$item" | jq -r '.dns_domain_cname // ""')
    [ -z "$cname" ] && continue

    if is_exempt_name "$cname"; then
      info "DNS [$cname] 在豁免列表 => 跳过创建"
      continue
    fi

    proxied=$(echo "$item" | jq -r '.dns_proxied // "false"')
    [ "$proxied" != "true" ] && proxied="false"

    record_ip="$external_ip"
    if [ -z "$record_ip" ]; then
      warning "record_ip 为空，无法为 [$cname] 创建 DNS 记录"
      continue
    fi

    info "创建 DNS：$cname.$PRIMARY_DOMAIN => IP=$record_ip, proxied=$proxied"
    local resp
    resp=$(dns_api_create "$cname" "$record_ip" "$proxied")
    if echo "$resp" | jq -e '.success == true' >/dev/null 2>&1; then
      ((count++))
      success "DNS 创建成功 => $cname.$PRIMARY_DOMAIN => $record_ip"
    else
      warning "DNS 创建失败 => $cname => $resp"
    fi
  done < <(jq -c '.tunnels[]' "$TEMP_FILE_FIXED")

  success "DNS 创建结束，共创建记录: $count"
  return 0
}

# ------------------ F.5 创建隧道（HTTP/HTTPS & TCP/UDP） ------------------

tunnel_create_http_https() {
  local node_name="$1"
  local tunnel_name="$2"
  local original_tunnel_name="$2"
  local local_ip="$3"
  local local_port="$4"
  local port_type="$5"
  local band_domain="$6"
  local encryption="$7"
  local compression="$8"
  local extraparams="$9"

  # 获取有效 token
  local token
  token=$(get_access_token) || {
    warning "无法获取 access_token，跳过创建隧道 => $tunnel_name"
    return 1
  }

  [ -z "$local_ip" ] && local_ip="127.0.0.1"
  [ -z "$encryption" ] && encryption="false"
  [ -z "$compression" ] && compression="false"
  [ -z "$extraparams" ] && extraparams=""

  info "创建 HTTP/HTTPS 隧道 => name=$tunnel_name, type=$port_type, local_ip=$local_ip, local_port=$local_port, domain=$band_domain"

  local payload
  payload=$(jq -n \
    --arg tk "$token" \
    --arg nm "$tunnel_name" \
    --arg nd "$node_name" \
    --arg lip "$local_ip" \
    --arg pt "$port_type" \
    --argjson lpt "$local_port" \
    --arg bd "$band_domain" \
    --arg enc "$encryption" \
    --arg com "$compression" \
    --arg ep "$extraparams" '
    {
      "token":       $tk,
      "tunnelname":  $nm,
      "node":        $nd,
      "localip":     $lip,
      "porttype":    $pt,
      "localport":   $lpt,
      "banddomain":  $bd,
      "encryption":  ($enc == "true"),
      "compression": ($com == "true"),
      "extraparams": $ep
    }')

  info "[隧道请求 Payload] $payload"
  local resp
  resp=$(tunnel_api_create "$payload")
  info "[接口响应] $resp"

  local code
  code=$(echo "$resp" | jq -r '.code // 0')
  if [ "$code" -eq 200 ]; then
    ((TUNNELS_CREATED++))
    ((TUNNELS_CHANGE++))
    success "HTTP/HTTPS 隧道创建成功 => $tunnel_name"
  elif echo "$resp" | grep -Eqi 'already exists|已存在|proxy .* already exists'; then
    local existing_id
    existing_id="$(tunnel_id_by_name "$tunnel_name" 2>/dev/null || true)"

    if [ -n "$existing_id" ]; then
      info "HTTP/HTTPS 隧道同名已存在，先删除旧隧道再重试 => name=$tunnel_name id=$existing_id"
      if tunnel_api_delete "$existing_id" >/dev/null; then
        record_sync_issue "$original_tunnel_name" "name_conflict_deleted" "deleted_existing_tunnel_id=$existing_id"
        resp=$(tunnel_api_create "$payload")
        info "[重试响应] $resp"
        code=$(echo "$resp" | jq -r '.code // 0')
        if [ "$code" -eq 200 ]; then
          ((TUNNELS_CREATED++))
          ((TUNNELS_CHANGE++))
          success "HTTP/HTTPS 隧道重建成功 => $tunnel_name"
          return 0
        fi
      else
        warning "删除同名旧隧道失败，将尝试自动改名重试 => $tunnel_name"
      fi
    fi

    local unique_name
    unique_name="$(make_unique_tunnel_name "$tunnel_name")"
    info "HTTP/HTTPS 隧道名称冲突，自动改名 => $tunnel_name -> $unique_name"
    record_sync_issue "$original_tunnel_name" "name_conflict_renamed" "rename_to=$unique_name"
    set_tunnel_name_override "$tunnel_name" "$unique_name"
    tunnel_name="$unique_name"
    payload="$(echo "$payload" | jq --arg nm "$tunnel_name" '.tunnelname = $nm')"
    resp=$(tunnel_api_create "$payload")
    info "[改名重试响应] $resp"
    code=$(echo "$resp" | jq -r '.code // 0')
    if [ "$code" -eq 200 ]; then
      ((TUNNELS_CREATED++))
      ((TUNNELS_CHANGE++))
      success "HTTP/HTTPS 隧道改名后创建成功 => $tunnel_name"
      return 0
    fi

    warning "HTTP/HTTPS 隧道名称冲突且改名后仍失败 => $tunnel_name"
    record_sync_issue "$original_tunnel_name" "name_conflict_retry_failed" "resp=$resp"
    return 1
  else
    local msg
    msg=$(echo "$resp" | jq -r '.msg // "未知错误"')
    # 检测隧道上限错误
    if [ "$code" -eq 400 ] && echo "$msg" | grep -q "隧道数量超过限制"; then
      error "触发隧道上限错误 (16个)，无法创建更多隧道: $msg"
      return 2  # 特殊状态码，表示触发了隧道上限
    fi
    warning "创建 HTTP/HTTPS 隧道失败 => code=$code => $msg"
  fi
}

tunnel_create_tcp_udp() {
  local node_name="$1"
  local cname="$2"
  local original_cname="$2"
  local local_ip="$3"
  local local_port="$4"
  local remote_port="$5"
  local port_type="$6"
  local encryption="$7"
  local compression="$8"
  local extraparams="$9"

  # 获取有效 token
  local token
  token=$(get_access_token) || {
    warning "无法获取 access_token，跳过创建隧道 => $cname"
    return 1
  }

  [ -z "$local_ip" ] && local_ip="127.0.0.1"
  [ -z "$encryption" ] && encryption="false"
  [ -z "$compression" ] && compression="false"
  [ -z "$extraparams" ] && extraparams=""

  local tunnel_name="$cname"  # 兼容：第3个参数直接作为 tunnelname（不再强制拼 _tcp/ _udp）
  info "创建 TCP/UDP 隧道 => name=$tunnel_name, type=$port_type, local_ip=$local_ip, local_port=$local_port, remote_port=$remote_port"

  local payload
  payload=$(jq -n \
    --arg tk "$token" \
    --arg nm "$tunnel_name" \
    --arg nd "$node_name" \
    --arg lip "$local_ip" \
    --arg pt "$port_type" \
    --argjson lpt "$local_port" \
    --argjson rpt "$remote_port" \
    --arg enc "$encryption" \
    --arg com "$compression" \
    --arg ep "$extraparams" '
    {
      "token":       $tk,
      "tunnelname":  $nm,
      "node":        $nd,
      "localip":     $lip,
      "porttype":    $pt,
      "localport":   $lpt,
      "remoteport":  $rpt,
      "encryption":  ($enc == "true"),
      "compression": ($com == "true"),
      "extraparams": $ep
    }')

  info "[隧道请求 Payload] $payload"
  local resp
  resp=$(tunnel_api_create "$payload")
  info "[接口响应] $resp"

  local code
  code=$(echo "$resp" | jq -r '.code // 0')
  if [ "$code" -eq 200 ]; then
    ((TUNNELS_CREATED++))
    ((TUNNELS_CHANGE++))
    success "TCP/UDP 隧道创建成功 => $tunnel_name"
  elif echo "$resp" | grep -Eqi 'already exists|已存在|proxy .* already exists'; then
    local existing_id
    existing_id="$(tunnel_id_by_name "$tunnel_name" 2>/dev/null || true)"

    if [ -n "$existing_id" ]; then
      info "TCP/UDP 隧道同名已存在，先删除旧隧道再重试 => name=$tunnel_name id=$existing_id"
      if tunnel_api_delete "$existing_id" >/dev/null; then
        record_sync_issue "$original_cname" "name_conflict_deleted" "deleted_existing_tunnel_id=$existing_id"
        resp=$(tunnel_api_create "$payload")
        info "[重试响应] $resp"
        code=$(echo "$resp" | jq -r '.code // 0')
        if [ "$code" -eq 200 ]; then
          ((TUNNELS_CREATED++))
          ((TUNNELS_CHANGE++))
          success "TCP/UDP 隧道重建成功 => $tunnel_name"
          return 0
        fi
      else
        warning "删除同名旧隧道失败，将尝试自动改名重试 => $tunnel_name"
      fi
    fi

    local unique_name
    unique_name="$(make_unique_tunnel_name "$tunnel_name")"
    info "TCP/UDP 隧道名称冲突，自动改名 => $tunnel_name -> $unique_name"
    record_sync_issue "$original_cname" "name_conflict_renamed" "rename_to=$unique_name"
    set_tunnel_name_override "$tunnel_name" "$unique_name"
    tunnel_name="$unique_name"
    payload="$(echo "$payload" | jq --arg nm "$tunnel_name" '.tunnelname = $nm')"
    resp=$(tunnel_api_create "$payload")
    info "[改名重试响应] $resp"
    code=$(echo "$resp" | jq -r '.code // 0')
    if [ "$code" -eq 200 ]; then
      ((TUNNELS_CREATED++))
      ((TUNNELS_CHANGE++))
      success "TCP/UDP 隧道改名后创建成功 => $tunnel_name"
      return 0
    fi

    warning "TCP/UDP 隧道名称冲突且改名后仍失败 => $tunnel_name"
    record_sync_issue "$original_cname" "name_conflict_retry_failed" "resp=$resp"
    return 1
  else
    local msg
    msg=$(echo "$resp" | jq -r '.msg // "未知错误"')
    # 检测隧道上限错误
    if [ "$code" -eq 400 ] && echo "$msg" | grep -q "隧道数量超过限制"; then
      error "触发隧道上限错误 (16个)，无法创建更多隧道: $msg"
      return 2  # 特殊状态码，表示触发了隧道上限
    fi
    warning "创建 TCP/UDP 隧道失败 => code=$code => $msg"
  fi
}

tunnel_create_from_current_state() {
  info "根据固定配置批量创建隧道"
  local node_name="$1"

  if [ -z "$node_name" ] || [ "$node_name" = "已过期" ]; then
    warning "节点名无效: $node_name => 跳过隧道创建"
    return 0
  fi

  if [ ! -f "$TEMP_FILE_FIXED" ]; then
    error "当前状态文件不存在: $TEMP_FILE_FIXED"
    return 1
  fi

  local observed_snapshot_lines=""
  if [ -f "$TUNNEL_LIST_FORMATTED" ]; then
    observed_snapshot_lines="$(snapshot_tunnel_line_list "$TUNNEL_LIST_FORMATTED" 2>/dev/null || true)"
  fi

  while IFS= read -r item; do
    local name ip local_port rport ttype cname proxied
    name=$(echo "$item"       | jq -r '.name // ""')
    ip=$(echo "$item"         | jq -r '.tunnel_local_ip // "127.0.0.1"')
    local_port=$(echo "$item" | jq -r '.tunnel_local_port // "80"')
    rport=$(echo "$item"      | jq -r '.tunnel_remote_port // ""')
    ttype=$(echo "$item"      | jq -r '.tunnel_type // "http"')
    cname=$(echo "$item"      | jq -r '.dns_domain_cname // ""')
    proxied=$(echo "$item"    | jq -r '.dns_proxied // "false"')
    local runtime_name
    runtime_name=$(echo "$item" | jq -r '.runtime_name // ""')

    [ -z "$name" ] && continue

    if is_exempt_name "$name"; then
      info "隧道 [$name] 在豁免列表 => 跳过创建"
      continue
    fi

    if [[ "$ttype" == "tcp" || "$ttype" == "udp" ]] && [ -z "$rport" ]; then
      warning "TCP/UDP 条目 [$name] 未指定 tunnel_remote_port => 跳过"
      continue
    fi

    if [ -z "$cname" ] || [ "$cname" = "null" ]; then
      cname="$name"
    fi

    if [ -z "$runtime_name" ] || [ "$runtime_name" = "null" ]; then
      runtime_name="$(build_tunnel_runtime_name "$name" "$local_port")"
    fi

    local desired_line
    desired_line=$(printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$(sanitize_tunnel_name "$runtime_name")" \
      "$ip" \
      "$local_port" \
      "$ttype" \
      "$rport" \
      "$(sanitize_tunnel_name "$cname")" \
      "$proxied")

    if [ -n "$observed_snapshot_lines" ] && printf '%s\n' "$observed_snapshot_lines" | grep -Fqx "$desired_line"; then
      info "隧道 [$name] 当前远端快照已匹配，跳过创建"
      continue
    fi

    local band_domain="${cname}.${PRIMARY_DOMAIN}"
    local encryption="false"
    local compression="false"
    local extra=""

    case "$ttype" in
      http|https)
        if ! tunnel_create_http_https \
          "$node_name" \
          "$runtime_name" \
          "$ip" \
          "$local_port" \
          "$ttype" \
          "$band_domain" \
          "$encryption" \
          "$compression" \
          "$extra"; then
          exit_code=$?
          if [ $exit_code -eq 2 ]; then
            error "隧道创建触发上限错误，停止创建剩余隧道"
            return 2
          fi
        fi
        ;;
      tcp|udp)
        if ! tunnel_create_tcp_udp \
          "$node_name" \
          "$runtime_name" \
          "$ip" \
          "$local_port" \
          "$rport" \
          "$ttype" \
          "$encryption" \
          "$compression" \
          "$extra"; then
          exit_code=$?
          if [ $exit_code -eq 2 ]; then
            error "隧道创建触发上限错误，停止创建剩余隧道"
            return 2
          fi
        fi
        ;;
      *)
        warning "未知隧道类型: $ttype => 跳过 [$name]"
        ;;
    esac
  done < <(jq -c '.tunnels[]' "$TEMP_FILE_FIXED")

  success "隧道创建结束，共创建 $TUNNELS_CREATED 条隧道"
  return 0
}

##############################################################################
# (G) frpc 配置 & Docker 容器
##############################################################################

get_tunnel_info() {
  local node_name="$1"
  if [ -z "$node_name" ]; then
    error "get_tunnel_info: 缺少节点名参数"
    return 1
  fi

  if $DRY_RUN; then
    info "[DRY_RUN][API][tunnel_config][REQUEST] method=GET url=https://cf-v2.uapis.cn/tunnel_config?token=<masked>&node=$(urlencode "$node_name") output=$FRPC_CONFIG_PATH"
    return 0
  fi

  # 获取有效 token
  local token
  token=$(get_access_token) || {
    error "无法获取 access_token"
    return 1
  }

  local node_name_ENCODED
  node_name_ENCODED=$(urlencode "$node_name")

  local url="https://cf-v2.uapis.cn/tunnel_config?token=$token&node=$node_name_ENCODED"
  api_log_request "tunnel_config" "GET" "https://cf-v2.uapis.cn/tunnel_config?token=$(mask_token "$token")&node=$node_name_ENCODED" "query_token"

  local response
  response=$(curl -sS -L --connect-timeout 10 --max-time 30 "$url")
  api_log_response "tunnel_config" "$response"
  if [ -z "$response" ]; then
    error "请求失败，未获取到任何响应"
    return 1
  fi
  if ! echo "$response" | jq empty >/dev/null 2>&1; then
    error "API 返回非 JSON，无法解析 => $response"
    return 1
  fi

  local code msg conf_data
  code=$(echo "$response" | jq -r '.code // 0')
  msg=$(echo "$response" | jq -r '.msg // "无msg"')
  conf_data=$(echo "$response" | jq -r '.data // ""')

  if [ "$code" != "200" ]; then
    if [ "$code" = "404" ]; then
      error "获取节点配置失败 => code=404 ($msg)"
      error "原因：节点上没有隧道，请先创建隧道后再获取配置"
      error "或者检查 Token 是否已过期（运行 'chmlfrp.sh oauth_reauth'）"
    else
      error "获取节点配置失败 => code=$code, msg=$msg"
    fi
    return 1
  fi
  if [ -z "$conf_data" ] || [ "$conf_data" = "null" ]; then
    error "API 返回的配置内容为空 => 无法写入 frpc.toml"
    return 1
  fi

  info "写入新的 frpc 配置到 => $FRPC_CONFIG_PATH"
  if echo "$conf_data" > "$FRPC_CONFIG_PATH"; then
    success "成功写入 => $FRPC_CONFIG_PATH"
    info "配置文件前 10 行预览："
    head -n 10 "$FRPC_CONFIG_PATH"
    return 0
  else
    error "写入 $FRPC_CONFIG_PATH 失败"
    return 1
  fi
}

update_frpc_docker() {
  local node_name="$1"
  if [ -z "$node_name" ]; then
    error "update_frpc_docker: 缺少节点名参数"
    return 1
  fi

  if $DRY_RUN; then
    info "[DRY_RUN] 将使用 $FRPC_CONFIG_PATH 重建 frpc 容器 [$FRPC_DOCKER_NAME]，目标节点: $node_name"
    return 0
  fi

  info "准备更新 frpc 容器，目标节点: $node_name"
  info "- 容器名: $FRPC_DOCKER_NAME"
  info "- 镜像名: $FRPC_DOCKER_IMAGE"

  if docker ps -a --format '{{.Names}}' | grep -qw "$FRPC_DOCKER_NAME"; then
    info "检测到旧容器 [$FRPC_DOCKER_NAME]，先正常停止再重建"
    if docker ps --format '{{.Names}}' | grep -qw "$FRPC_DOCKER_NAME"; then
      info "先执行 docker stop，避免强制删除导致未正常关闭"
      if docker stop -t "${FRPC_STOP_TIMEOUT:-10}" "$FRPC_DOCKER_NAME" >/dev/null 2>&1; then
        success "旧容器已正常停止"
      else
        warning "旧容器 stop 失败，继续执行删除"
      fi
    fi
    if docker rm "$FRPC_DOCKER_NAME" >/dev/null 2>&1; then
      success "旧容器已删除"
    else
      warning "删除旧容器失败（忽略）"
    fi
  fi

  if ! docker images -q "$FRPC_DOCKER_IMAGE" >/dev/null 2>&1; then
    info "本地无镜像，开始 docker pull..."
    if ! docker pull "$FRPC_DOCKER_IMAGE"; then
      error "镜像拉取失败，无法继续"
      return 1
    fi
  fi

  info "创建并启动新容器..."
  if ! docker run -d \
        --name "$FRPC_DOCKER_NAME" \
        --net bridge \
        --pids-limit 2048 \
        -e TZ="Asia/Shanghai" \
        -v "${FRPC_CONFIG_PATH}:/etc/frp/frpc.toml:rw" \
        "$FRPC_DOCKER_IMAGE" >/dev/null; then
    error "docker run 失败"
    return 1
  fi

  success "frpc 容器已启动完成"
}

classify_post_restart_proxy_state() {
  local node_name="$1"
  local settle_seconds="${POST_RESTART_SETTLE_SECONDS:-8}"
  local log_tail="${POST_RESTART_LOG_TAIL:-120}"

  [ -n "$settle_seconds" ] || settle_seconds=8
  [ -n "$log_tail" ] || log_tail=120

  sleep "$settle_seconds"

  local logs
  logs=$(docker logs --tail "$log_tail" "$FRPC_DOCKER_NAME" 2>/dev/null || true)
  [ -n "$logs" ] || return 0

  if ! printf '%s\n' "$logs" | grep -q 'login to server success'; then
    return 0
  fi

  local conflict_count
  conflict_count=$(count_recent_proxy_conflicts "$logs")
  if [ "$conflict_count" -lt "$NODE_PROXY_CONFLICT_THRESHOLD" ]; then
    return 0
  fi

  local classification detail
  if printf '%s\n' "$logs" | grep -Eq 'router config conflict|route config conflict'; then
    classification="router_conflict"
    detail="post_restart_router_conflict_count=$conflict_count"
  elif printf '%s\n' "$logs" | grep -Eq 'proxy \[.*\] already exists'; then
    classification="node_proxy_already_exists"
    detail="post_restart_proxy_already_exists_count=$conflict_count"
  else
    classification="node_unusable"
    detail="post_restart_proxy_error_count=$conflict_count"
  fi

  record_sync_issue "$node_name" "$classification" "$detail"
  write_sync_result failed "$classification" "$detail" "$node_name" "$detail"
  return 11
}

cleanup_temp_files() {
  info "跳过清理缓存快照文件：保留固定隧道、隧道列表、DNS 列表快照，便于排障和后续 AI/人工接手"
  success "缓存快照已保留"
}

##############################################################################
# (H) 主流程 main()
##############################################################################

main() {
  parse_args "$@"
  load_settings
  init_paths

  info "========== ChmlFrp 新修复流程脚本启动 =========="
  local script_path
  script_path=$(readlink -f "$0" 2>/dev/null || echo "$0")
  info "脚本路径: $script_path"
  info "当前时间: $(date '+%Y-%m-%d %H:%M:%S'), 主机 IP: $HOST_IP"
  info "固定隧道文件将使用: $FIXED_TUNNEL_FILE"
  $DRY_RUN && info "当前为 DRY_RUN 模式：不会对实际资源产生改动"

  # 步骤一：读取全局配置
  if ! read_configs_from_userdata; then
    error "读取全局配置失败 => 无法继续"
    exit 1
  fi
  success "步骤一：读取全局配置完成"

  # 步骤二：前置条件检查（仅在标准全量模式 & 非 force-run 下生效；只做预检，不在这里直接退出）
  if ! $DNS_ONLY && ! $CLEAN_INVALID_ONLY && ! $FORCE_RUN; then
    check_source_ts
  else
    info "跳过前置 mtime/在线测试/frpc 日志检查：DNS_ONLY=$DNS_ONLY, CLEAN_INVALID_ONLY=$CLEAN_INVALID_ONLY, FORCE_RUN=$FORCE_RUN"
  fi

  # 步骤三：同步用户详情（主要用于记录用户信息；V2 删除隧道已不依赖 userid）
  if check_json_file "$TUNNEL_USERDATA_FILE" && read_userdetail_id; then
    success "步骤三：解析 CHMLFRP_USERID 完成 => $CHMLFRP_USERID"
  else
    warning "步骤三：首次读取用户详情失败，尝试自动同步后再解析"
    if read_userdetail_id; then
      success "步骤三：自动同步后解析 CHMLFRP_USERID 完成 => $CHMLFRP_USERID"
    else
      warning "步骤三：仍未能解析 CHMLFRP_USERID，但这不会阻塞 V2 删除接口测试"
    fi
  fi

  # 步骤四：读取当前 frpc 节点 IP
  if read_current_node_ip; then
    success "步骤四：当前 frpc 节点 IP = $CURRENT_NODE_IP"
  else
    warning "步骤四：无法获取当前节点 IP => 后续更多依赖测速选优"
  fi

  # 步骤五：生成当前状态快照（固定隧道 + 隧道列表 + DNS 列表）
  if ! tunnel_get_fixed; then
    error "固定隧道读取失败 => 退出"
    exit 1
  fi

  local fixed_count
  fixed_count=$(jq '.tunnels | length' "$TEMP_FILE_FIXED" 2>/dev/null || echo 0)
  if [ "$fixed_count" -le 0 ]; then
    error "固定隧道清单为空（$FIXED_TUNNEL_FILE），出于安全考虑终止（避免误删/误改）"
    exit 1
  fi
  info "固定隧道清单条目数: $fixed_count"

  if ! tunnel_fetch_list_local; then
    error "获取隧道列表失败，无法确定现有隧道状态，终止全量同步流程"
    error "提示：请优先检查 Bearer token 调用方式是否与面板抓包一致"
    exit 1
  fi

  if ! tunnel_format_list; then
    error "隧道列表格式化失败 => 退出"
    exit 1
  fi

  if ! dns_fetch_list_local; then
    warning "获取 DNS 列表失败，将视为当前无 DNS"
    write_json_file "$DNS_FORMATTED_FILE" "$(jq -n --argjson ts "$(date +%s)" '{refreshed_at:$ts, total:0, records:[]}')"
    info "当前 Cloudflare DNS 列表为空 (0 条)"
  else
    if ! dns_format_list; then
      error "DNS 列表格式化失败 => 退出"
      exit 1
    fi
  fi

  success "步骤五：当前状态快照生成完成"

  local desired_fp observed_fp
  desired_fp=$(tunnel_snapshot_fingerprint "$TEMP_FILE_FIXED" 2>/dev/null || echo "")
  observed_fp=$(tunnel_snapshot_fingerprint "$TUNNEL_LIST_FORMATTED" 2>/dev/null || echo "")

  info "隧道期望快照 hash: ${desired_fp:0:12}..."
  info "隧道实际快照 hash: ${observed_fp:0:12}..."

  if $SOURCE_PRECHECK_OK && [ -n "$desired_fp" ] && [ "$desired_fp" = "$observed_fp" ]; then
    success "预检通过且远端隧道快照与期望一致 ➜ 无需执行修复流程，脚本直接退出。"
    write_sync_result success noop "precheck_ok_and_snapshot_match"
    cleanup_temp_files
    write_source_ts_cache
    exit 0
  fi

  if [ -n "$desired_fp" ] && [ -n "$observed_fp" ] && [ "$desired_fp" != "$observed_fp" ]; then
    info " - 远端隧道快照与固定清单不一致，将继续执行修复流程"
    local desired_lines_file observed_lines_file missing_count extra_count missing_preview extra_preview
    desired_lines_file="$LOG_DIR/.desired-tunnels.$$"
    observed_lines_file="$LOG_DIR/.observed-tunnels.$$"
    snapshot_tunnel_line_list "$TEMP_FILE_FIXED" > "$desired_lines_file" 2>/dev/null || true
    snapshot_tunnel_line_list "$TUNNEL_LIST_FORMATTED" > "$observed_lines_file" 2>/dev/null || true
    missing_count=$(comm -23 "$desired_lines_file" "$observed_lines_file" 2>/dev/null | wc -l | tr -d ' ')
    extra_count=$(comm -13 "$desired_lines_file" "$observed_lines_file" 2>/dev/null | wc -l | tr -d ' ')
    missing_preview=$(comm -23 "$desired_lines_file" "$observed_lines_file" 2>/dev/null | head -n 5 | tr '\n' '; ')
    extra_preview=$(comm -13 "$desired_lines_file" "$observed_lines_file" 2>/dev/null | head -n 5 | tr '\n' '; ')
    info " - 缺少期望隧道数量: ${missing_count:-0}"
    info " - 额外隧道数量: ${extra_count:-0}"
    [ -n "$missing_preview" ] && info " - 缺少示例: $missing_preview"
    [ -n "$extra_preview" ] && info " - 额外示例: $extra_preview"
    record_sync_issue "__snapshot__" "tunnel_drift" "missing=${missing_count:-0} extra=${extra_count:-0}"
    rm -f "$desired_lines_file" "$observed_lines_file" 2>/dev/null || true
  fi

  if ! $SOURCE_PRECHECK_OK; then
    info " - 预检未完全通过，将继续执行修复流程"
  fi


   # 步骤六：节点选择策略（仅当后续需要 BEST_NODE 时才执行）
  if ! $CLEAN_INVALID_ONLY; then
    if [ -n "$FORCE_NODE_NAME" ]; then
      info "步骤六：节点策略 => 使用指定节点（--node="$FORCE_NODE_NAME"）"
      if ! select_node_by_name "$FORCE_NODE_NAME"; then
        error "步骤六：指定节点校验失败 => $FORCE_NODE_NAME"
        exit 1
      fi
      info "步骤六：指定节点通过校验 => 名称=$BEST_NODE_NAME, IP=$BEST_NODE_IP"
    elif $PREFER_FRPC_NODE; then
      info "步骤六：节点策略 => 尝试优先使用当前 frpc 节点（需 nodeinfo 在线 + ping 通过），否则退回 nodeinfo + ping 选优"
      if prefer_node_from_frpc; then
        info "步骤六：已优先选用当前 frpc 节点 => 名称=$BEST_NODE_NAME, IP=$BEST_NODE_IP"
      else
        info "步骤六：当前 frpc 节点未通过在线/稳定性校验，改为在所有在线节点中按 ping 选最优"
        if ! select_best_node; then
          error "步骤六：无法根据 nodeinfo / ping 选出可用节点 => 退出"
          exit 1
        fi
        info "步骤六：最终选定目标节点（全局选优） => 名称=$BEST_NODE_NAME, IP=$BEST_NODE_IP"
      fi
    else
      info "步骤六：节点策略 => 统一在 nodeinfo 在线节点中按 ping 选出延迟最优节点"
      if ! select_best_node; then
        error "步骤六：无法根据 nodeinfo / ping 选出可用节点 => 退出"
        exit 1
      fi
      info "步骤六：最终选定目标节点 => 名称=$BEST_NODE_NAME, IP=$BEST_NODE_IP"
    fi
  else
    info "步骤六：当前为 --clean-invalid 模式，无需节点测速与选优，直接进入增量清理逻辑"
  fi



  # 步骤七：按模式执行

  # 7.1 仅清理无效隧道 / DNS
  if $CLEAN_INVALID_ONLY; then
    info "步骤七：模式 = 仅清理无效隧道 / DNS（不新建，不更新 frpc）"
    if ! clean_invalid_resources; then
      write_sync_result failed clean_invalid_failed "clean_invalid_resources_failed"
      error "clean_invalid_resources 执行失败"
      exit 1
    fi
    write_sync_result success clean_invalid_only "clean_invalid_resources_completed"
    cleanup_temp_files
    write_source_ts_cache
    success "========== 脚本结束：仅清理无效隧道 / DNS 模式 =========="
    exit 0
  fi

  # 7.2 仅同步 DNS
  if $DNS_ONLY; then
    info "步骤七：模式 = 仅同步 DNS（不改隧道，不更新 frpc）"
    if ! delete_non_exempt_dns; then
      warning "删除非豁免 DNS 时出现问题，继续尝试创建新记录"
    fi
    if ! dns_create_from_current_state "$BEST_NODE_IP"; then
      write_sync_result failed dns_only_failed "dns_create_from_current_state_failed"
      error "dns_create_from_current_state 执行失败"
      exit 1
    fi
    write_sync_result success dns_only "dns_create_from_current_state_completed"
    cleanup_temp_files
    write_source_ts_cache
    success "========== 脚本结束：仅同步 DNS 模式 =========="
    exit 0
  fi

  # 7.3 标准全量修复模式
  info "步骤七：模式 = 全量同步到目标节点 [$BEST_NODE_NAME / $BEST_NODE_IP]"

  # 1) 全量删除非豁免隧道 / DNS
  if ! perform_force_delete_all; then
    write_sync_result failed cleanup_failed "perform_force_delete_all_failed" "$BEST_NODE_NAME"
    error "删除非豁免资源失败，终止全量同步流程"
    error "提示：如果是因为无法获取隧道列表（Token 失效），请运行 'chmlfrp.sh oauth_reauth' 重新获取 Token"
    exit 1
  fi

  # 2) 重建 DNS
  if ! dns_create_from_current_state "$BEST_NODE_IP"; then
    warning "DNS 创建/更新过程中出现错误，请稍后手动核查"
  fi

  # 3) 重建隧道
  if ! tunnel_create_from_current_state "$BEST_NODE_NAME"; then
    exit_code=$?
    if [ $exit_code -eq 2 ]; then
      write_sync_result failed tunnel_limit "tunnel_limit_reached" "$BEST_NODE_NAME"
      error "隧道创建触发了上限错误（16个），当前隧道数量已达上限"
      error "请手动登录 ChmlFrp 控制台删除不需要的隧道后重试"
    else
      write_sync_result failed tunnel_create_failed "tunnel_create_from_current_state_failed" "$BEST_NODE_NAME"
      error "隧道创建过程中出现错误，请稍后手动核查"
    fi
    exit 1
  fi

  # 4) 更新 frpc 配置并重启容器
  if ! get_tunnel_info "$BEST_NODE_NAME"; then
    write_sync_result failed tunnel_config_failed "get_tunnel_info_failed" "$BEST_NODE_NAME"
    error "获取 frpc 配置失败，终止（避免用错误配置重启）"
    exit 1
  fi
  success "已获取并写入最新 frpc.toml"

  if ! update_frpc_docker "$BEST_NODE_NAME"; then
    write_sync_result failed frpc_restart_failed "update_frpc_docker_failed" "$BEST_NODE_NAME"
    error "更新 frpc 容器失败"
    exit 1
  fi
  success "frpc 容器已重启并指向目标节点 [$BEST_NODE_NAME]"

  classify_post_restart_proxy_state "$BEST_NODE_NAME"
  exit_code=$?
  if [ "$exit_code" -eq 11 ]; then
    error "重启后检测到节点级代理冲突，判定当前节点不可用：$BEST_NODE_NAME"
    exit 1
  elif [ "$exit_code" -ne 0 ]; then
    error "重启后代理状态检测失败：$BEST_NODE_NAME"
    exit 1
  fi

  # 收尾
  write_sync_result success full_sync_completed "full_sync_completed" "$BEST_NODE_NAME"
  cleanup_temp_files
  write_source_ts_cache
  success "========== 脚本执行完成（全量同步模式） =========="
  exit 0
}

main "$@"
