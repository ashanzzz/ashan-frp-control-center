# ChmlFrp OAuth2 认证支持修改计划

> 文档版本：1.0  
> 更新日期：2026-04-07  
> 目标：为 ChmlFrp 脚本套件添加 QZhua OAuth2 认证支持

---

## 一、现状分析

### 1.1 当前 Token 处理方式

当前代码使用**旧版 ChmlFrp 长期 Token 机制**：

| 特性 | 当前实现 |
|------|----------|
| Token 来源 | `userdata.txt` 的 `chmlfrp.token` 字段 |
| Token 格式 | 简单字符串（如 `E3g7Q9o3RHppxJPilOGyLC5v`） |
| 有效期 | 理论上永久有效 |
| 刷新机制 | ❌ 无，需手动更换 |
| 认证参数 | 仅 Token，无其他凭证 |

### 1.2 代码中 Token 使用位置

#### chmlfrp.sh

| 函数 | 行号 | 用途 |
|------|------|------|
| `userinfo_sync()` | 298 | 使用 username/password 登录获取用户信息 |
| `nodeinfo_get()` | 446 | 拼接 token 调用 nodeinfo API |
| `select_best_node()` | 536 | 读取 `chmlfrp.token` |
| `network_diagnosis()` | 456 | 测试 API 连通性 |

#### new_fix_flow.sh

| 函数 | 行号 | 用途 |
|------|------|------|
| `read_configs_from_userdata()` | 298 | 读取 `chmlfrp.token` |
| `tunnel_api_create()` | 927 | 创建隧道时发送 token |
| `tunnel_api_delete()` | 947 | 删除隧道时发送 token |
| `tunnel_api_fetch_list()` | 986 | 获取隧道列表时发送 token |
| `get_tunnel_info()` | 1818 | 获取节点配置时发送 token |

### 1.3 新旧认证方式对比

| 对比项 | 旧方式 | QZhua OAuth2 |
|--------|--------|---------------|
| Token 类型 | 长期固定字符串 | access_token (短期) + refresh_token (长期) |
| Token 格式 | `E3g7Q9o3RHppxJPilOGyLC5v` | JWT 格式 (`eyJraWQiOiI3Mz...`) |
| 有效期 | 永久 | access_token: ~10分钟 (599秒) |
| 刷新方式 | 无 | 用 refresh_token 换取新 access_token |
| 必需参数 | 仅 token | client_id + client_secret + access_token |
| 获取流程 | 后台手动复制 | 设备码授权 → 浏览器授权 → 轮询获取 |

---

## 二、修改目标

### 2.1 功能目标

1. **支持 QZhua OAuth2 设备码授权流程**
2. **自动刷新 Token**：access_token 过期前自动用 refresh_token 刷新
3. **兼容旧 Token**：如果配置了旧格式 token，仍可继续使用（向后兼容）
4. **首次授权引导**：当没有有效 token 时，提示用户完成授权

### 2.2 修改范围

| 文件 | 修改内容 |
|------|----------|
| `chmlfrp.sh` | 新增 OAuth2 Token 管理函数 |
| `new_fix_flow.sh` | Token 获取和刷新逻辑 |
| `userdata.txt.example` | 新增 OAuth2 字段 |
| `settings.env` | 可选：OAuth2 配置项 |

---

## 三、详细设计

### 3.1 新增配置字段

#### userdata.txt 新结构

```json
{
  "cloudflare": {
    "email": "your@email.com",
    "api_token": "YOUR_CF_API_TOKEN",
    "zone_id": "YOUR_CF_ZONE_ID"
  },
  "chmlfrp": {
    "username": "YOUR_USERNAME",
    "password": "YOUR_PASSWORD",
    "token": "旧格式token（兼容用）",
    "oauth2": {
      "enabled": true,
      "client_id": "019d534218e67f8a862056c1efb869db",
      "client_secret": "0a98ee0b7c69daa4c4922bae9be5df95eff6",
      "access_token": "eyJraWQiOiI3MzBiZGRmNC...",
      "refresh_token": "KA3DLo9nxPBfZWNNuMB7gaP...",
      "token_expires_at": 1775554660,
      "token_file": "/path/to/.chmlfrp_token.json"
    }
  }
}
```

#### 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `oauth2.enabled` | boolean | 是否启用 OAuth2 认证 |
| `oauth2.client_id` | string | QZhua 客户端 ID |
| `oauth2.client_secret` | string | QZhua 客户端密钥 |
| `oauth2.access_token` | string | 当前有效的 access_token |
| `oauth2.refresh_token` | string | 用于刷新的 refresh_token |
| `oauth2.token_expires_at` | number | access_token 过期时间戳 |
| `oauth2.token_file` | string | （可选）Token 缓存文件路径 |

### 3.2 新增函数设计

#### chmlfrp.sh 新增函数

```bash
# ------------------ OAuth2 Token 管理 ------------------

# 检查是否启用 OAuth2
is_oauth2_enabled() {
  # 读取 oauth2.enabled 配置
  # 返回 0 表示启用，1 表示未启用
}

# 获取有效的 access_token（自动刷新）
# 返回有效的 access_token 字符串
# 内部逻辑：
#   1. 检查 access_token 是否存在
#   2. 检查是否过期（expires_at < 当前时间）
#   3. 如过期，用 refresh_token 刷新
#   4. 如无 refresh_token，提示用户重新授权
get_access_token() {
  # 读取 userdata.txt 中的 oauth2 配置
  # 检查 token 是否存在且未过期
  # 如需刷新，调用 refresh_access_token
  # 返回 access_token
}

# 刷新 access_token
refresh_access_token() {
  # 读取 refresh_token
  # 调用 QZhua token endpoint
  # 更新 userdata.txt 中的 access_token 和 expires_at
  # 保存新的 refresh_token（如果有rotation）
}

# 检查 token 是否需要刷新
is_token_expired() {
  # 读取 token_expires_at
  # 与当前时间比较
  # 提前 60 秒开始刷新（预留缓冲时间）
}

# 完整的设备码授权流程
# 用于首次授权或 token 全部失效后
device_code_authorization() {
  # 1. 调用 device_authorization 获取 device_code 和 user_code
  # 2. 打印授权 URL 和 user_code
  # 3. 轮询 token endpoint 直到获得 access_token
  # 4. 保存到 userdata.txt
}

# 引导用户完成授权
prompt_user_auth() {
  # 打印清晰的授权指引
  # 包含授权 URL 和操作步骤
}

# 测试 token 是否有效（调用简单 API 验证）
test_token() {
  # 调用一个轻量 API（如 /node）验证 token
  # 返回 0 表示有效，1 表示无效
}
```

### 3.3 Token 刷新策略

```
┌─────────────────────────────────────────────────────────────┐
│                    get_access_token()                        │
│                                                              │
│  ┌─────────────┐                                           │
│  │ 读取缓存Token │                                           │
│  └──────┬──────┘                                           │
│         │                                                   │
│         ▼                                                   │
│  ┌─────────────┐    是    ┌─────────────────┐              │
│  │ token存在？  │ ──────→ │ 返回 access_token │              │
│  └──────┬──────┘         └─────────────────┘              │
│         │ 否                                               │
│         ▼                                                   │
│  ┌─────────────────┐                                       │
│  │ 检查refresh_token│                                       │
│  └──────┬──────────┘                                       │
│         │                                                   │
│    ┌────┴────┐                                             │
│    │存在？    │                                             │
│    └────┬────┘                                             │
│      是 │                         否                        │
│      ┌──┴──┐                       ▼                       │
│      ▼      ▼              ┌─────────────────┐             │
│  ┌──────────────┐          │ 提示用户授权     │             │
│  │refresh_token │          └─────────────────┘             │
│  │  刷新Token  │                                         │
│  └──────┬──────┘                                         │
│         │                                                   │
│         ▼                                                   │
│  ┌─────────────┐    成功                                   │
│  │更新缓存文件  │ ──────→ 返回新的 access_token             │
│  └─────────────┘                                           │
│         │                                                   │
│         ▼ 失败                                             │
│  ┌─────────────────┐                                       │
│  │ 提示重新授权    │                                       │
│  └─────────────────┘                                       │
└─────────────────────────────────────────────────────────────┘
```

---

## 四、实施步骤

### 阶段一：基础设施（chmlfrp.sh）

#### 步骤 1.1：添加 OAuth2 配置常量

在 `chmlfrp.sh` 头部添加：

```bash
# --- QZhua OAuth2 配置 ---
QZHUA_API_BASE="https://account-api.qzhua.net"
QZHUA_TOKEN_ENDPOINT="${QZHUA_API_BASE}/oauth2/token"
QZHUA_DEVICE_CODE_ENDPOINT="${QZHUA_API_BASE}/oauth2/device_authorization"
QZHUA_SCOPE="chmlfrp_api"
TOKEN_EXPIRE_BUFFER=60  # 提前60秒刷新
```

#### 步骤 1.2：添加 OAuth2 管理函数

在 `chmlfrp.sh` 末尾（`main()` 函数之前）添加：

```bash
###############################################################################
# QZhua OAuth2 Token 管理
###############################################################################

is_oauth2_enabled() {
  if ! json_ok "$USERDATA_FILE"; then
    return 1
  fi
  local enabled
  enabled=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.enabled // false')
  [ "$enabled" = "true" ]
}

get_token_expires_at() {
  json_get "$USERDATA_FILE" '.chmlfrp.oauth2.token_expires_at // 0'
}

is_token_expired() {
  local expires_at now
  expires_at=$(get_token_expires_at)
  now=$(now_ts)
  [ "$expires_at" -gt 0 ] && [ $((now + TOKEN_EXPIRE_BUFFER)) -ge "$expires_at" ]
}

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
  fi
}

refresh_access_token() {
  require_cmd curl || { err "缺少 curl"; return 1; }
  
  local refresh_token
  refresh_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.refresh_token // empty')
  
  if [ -z "$refresh_token" ]; then
    err "没有 refresh_token，需要重新授权"
    return 1
  fi
  
  local client_id client_secret
  client_id=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.client_id // empty')
  client_secret=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.client_secret // empty')
  
  if [ -z "$client_id" ] || [ -z "$client_secret" ]; then
    err "缺少 client_id 或 client_secret"
    return 1
  fi
  
  info "正在刷新 access_token..."
  
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
    success "access_token 刷新成功"
    return 0
  else
    err "refresh_token 刷新失败: $(echo "$resp" | jq -r '.error // "未知错误"')"
    return 1
  fi
}

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
  local max_attempts=60  # 最多等待 5 分钟
  
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

get_access_token() {
  if ! is_oauth2_enabled; then
    # 兼容旧模式：直接返回旧格式 token
    json_get "$USERDATA_FILE" '.chmlfrp.token // empty'
    return 0
  fi
  
  local access_token
  access_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.access_token // empty')
  
  if [ -z "$access_token" ] || [ "$access_token" = "null" ]; then
    info "未找到 access_token，尝试刷新..."
    if refresh_access_token; then
      access_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.access_token // empty')
    else
      warn "刷新失败，尝试设备码授权..."
      device_code_auth
      access_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.access_token // empty')
    fi
  elif is_token_expired; then
    info "access_token 已过期，尝试刷新..."
    if ! refresh_access_token; then
      warn "刷新失败，尝试设备码授权..."
      device_code_auth
      access_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.access_token // empty')
    else
      access_token=$(json_get "$USERDATA_FILE" '.chmlfrp.oauth2.access_token // empty')
    fi
  fi
  
  echo "$access_token"
}
```

#### 步骤 1.3：修改 select_best_node 函数

将第 536 行：
```bash
token=$(json_get "$USERDATA_FILE" '.chmlfrp.token // empty')
```

替换为：
```bash
token=$(get_access_token)
```

同时确保调用前检查 token 有效性：
```bash
if [ -z "$token" ]; then
  _sbn_log ERROR "无法获取有效的 access_token"
  return 1
fi
```

#### 步骤 1.4：修改 nodeinfo_get 函数

当前函数直接使用传入的 token，建议保持不变（由调用者保证 token 有效）。

但可以添加可选的 token 过期检测：
```bash
nodeinfo_get() {
  local token="$1" name="$2"
  local enc url resp
  enc=$(urlencode "$name")
  url="http://cf-v2.uapis.cn/nodeinfo?token=${token}&node=${enc}"
  resp=$(curl -sS -L --connect-timeout 3 --max-time 6 "$url")
  
  # 检查是否返回 401
  local code
  code=$(echo "$resp" | jq -r '.code // 0')
  if [ "$code" -eq 401 ]; then
    # Token 无效，尝试刷新
    if refresh_access_token; then
      token=$(get_access_token)
      url="http://cf-v2.uapis.cn/nodeinfo?token=${token}&node=${enc}"
      resp=$(curl -sS -L --connect-timeout 3 --max-time 6 "$url")
    fi
  fi
  
  echo "$resp"
}
```

---

### 阶段二：修改 new_fix_flow.sh

#### 步骤 2.1：添加 OAuth2 Token 获取函数

在文件开头（变量定义之后，`init_paths()` 函数之前）添加：

```bash
##############################################################################
# (B.5) QZhua OAuth2 Token 管理
##############################################################################

# 读取配置中的 OAuth2 设置
read_oauth2_config() {
  if [ ! -f "$USERDATA_FILE" ]; then
    return 1
  fi
  
  OAUTH2_ENABLED=$(jq -r '.chmlfrp.oauth2.enabled // false' "$USERDATA_FILE")
  OAUTH2_CLIENT_ID=$(jq -r '.chmlfrp.oauth2.client_id // empty' "$USERDATA_FILE")
  OAUTH2_CLIENT_SECRET=$(jq -r '.chmlfrp.oauth2.client_secret // empty' "$USERDATA_FILE")
  OAUTH2_ACCESS_TOKEN=$(jq -r '.chmlfrp.oauth2.access_token // empty' "$USERDATA_FILE")
  OAUTH2_REFRESH_TOKEN=$(jq -r '.chmlfrp.oauth2.refresh_token // empty' "$USERDATA_FILE")
  OAUTH2_TOKEN_EXPIRES_AT=$(jq -r '.chmlfrp.oauth2.token_expires_at // 0' "$USERDATA_FILE")
}

# 检查 token 是否过期
is_token_expired() {
  local now
  now=$(date +%s)
  [ "$OAUTH2_TOKEN_EXPIRES_AT" -gt 0 ] && [ $((now + 60)) -ge "$OAUTH2_TOKEN_EXPIRES_AT" ]
}

# 刷新 access_token
refresh_access_token() {
  if [ -z "$OAUTH2_REFRESH_TOKEN" ] || [ -z "$OAUTH2_CLIENT_ID" ] || [ -z "$OAUTH2_CLIENT_SECRET" ]; then
    error "OAuth2 刷新参数不完整"
    return 1
  fi
  
  info "刷新 access_token..."
  
  local resp
  resp=$(curl -sS -X POST "https://account-api.qzhua.net/oauth2/token" \
    -u "${OAUTH2_CLIENT_ID}:${OAUTH2_CLIENT_SECRET}" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "grant_type=refresh_token" \
    -d "refresh_token=${OAUTH2_REFRESH_TOKEN}")
  
  if echo "$resp" | jq -e '.access_token' >/dev/null 2>&1; then
    local new_access_token new_refresh_token expires_in now expires_at
    new_access_token=$(echo "$resp" | jq -r '.access_token')
    new_refresh_token=$(echo "$resp" | jq -r '.refresh_token // "'"$OAUTH2_REFRESH_TOKEN"'"')
    expires_in=$(echo "$resp" | jq -r '.expires_in // 3600')
    now=$(date +%s)
    expires_at=$((now + expires_in))
    
    # 更新 userdata.txt
    local tmp_file="${USERDATA_FILE}.tmp"
    jq --arg at "$new_access_token" \
       --arg rt "$new_refresh_token" \
       --argjson ea "$expires_at" \
       '.chmlfrp.oauth2.access_token = $at | .chmlfrp.oauth2.refresh_token = $rt | .chmlfrp.oauth2.token_expires_at = $ea' \
       "$USERDATA_FILE" > "$tmp_file" && mv "$tmp_file" "$USERDATA_FILE"
    
    OAUTH2_ACCESS_TOKEN="$new_access_token"
    OAUTH2_REFRESH_TOKEN="$new_refresh_token"
    OAUTH2_TOKEN_EXPIRES_AT="$expires_at"
    
    success "access_token 刷新成功"
    return 0
  else
    error "刷新失败: $(echo "$resp" | jq -r '.error // "未知错误"')"
    return 1
  fi
}

# 获取有效的 access_token
get_access_token() {
  # 如果启用 OAuth2
  if [ "$OAUTH2_ENABLED" = "true" ]; then
    # 检查是否有 token
    if [ -z "$OAUTH2_ACCESS_TOKEN" ] || [ "$OAUTH2_ACCESS_TOKEN" = "null" ]; then
      info "未找到 access_token，尝试刷新..."
      refresh_access_token || {
        error "无法获取有效的 access_token"
        return 1
      }
    elif is_token_expired; then
      info "access_token 已过期，尝试刷新..."
      refresh_access_token || {
        error "无法刷新 access_token"
        return 1
      }
    fi
    
    echo "$OAUTH2_ACCESS_TOKEN"
  else
    # 兼容旧模式
    jq -r '.chmlfrp.token // empty' "$USERDATA_FILE"
  fi
}
```

#### 步骤 2.2：修改 API 调用函数

##### tunnel_api_create()

在函数开头获取有效 token：
```bash
tunnel_api_create() {
  local json_payload="$1"
  local url="http://cf-v2.uapis.cn/create_tunnel"
  
  # 获取有效 token
  local token
  token=$(get_access_token) || return 1
  
  # 将 token 加入 payload
  local tokenized_payload
  tokenized_payload=$(echo "$json_payload" | jq --arg t "$token" '.token = $t')
  
  if $DRY_RUN; then
    info "[DRY_RUN] 调用 create_tunnel: $tokenized_payload"
    echo '{"code":200,"msg":"DRY_RUN"}'
    return 0
  fi
  
  curl -sS -X POST "$url" \
    -H "Content-Type: application/json" \
    --data-raw "$tokenized_payload"
}
```

##### tunnel_api_fetch_list()

```bash
tunnel_api_fetch_list() {
  local token
  token=$(get_access_token) || {
    error "无法获取 access_token"
    echo '{"code":401,"msg":"无有效token"}'
    return 1
  }
  
  local url="http://cf-v2.uapis.cn/tunnel?token=${token}"
  info "拉取隧道列表: ${url}"
  curl -sS -X GET "${url}"
}
```

##### get_tunnel_info()

```bash
get_tunnel_info() {
  local node_name="$1"
  if [ -z "$node_name" ]; then
    error "get_tunnel_info: 缺少节点名参数"
    return 1
  fi
  
  # 获取有效 token
  local token
  token=$(get_access_token) || return 1
  
  if $DRY_RUN; then
    info "[DRY_RUN] 将获取节点 [$node_name] 的 frpc 配置并写入 $FRPC_CONFIG_PATH"
    return 0
  fi
  
  info "获取节点配置：节点=$node_name (Token前4位=${token:0:4})"
  
  local node_name_ENCODED
  node_name_ENCODED=$(urlencode "$node_name")
  
  local url="http://cf-v2.uapis.cn/tunnel_config?token=$token&node=$node_name_ENCODED"
  # ... 后续不变
}
```

#### 步骤 2.3：修改 read_configs_from_userdata()

在函数开头调用 `read_oauth2_config`：
```bash
read_configs_from_userdata() {
  info "开始读取配置文件 => $USERDATA_FILE"
  
  # 新增：读取 OAuth2 配置
  read_oauth2_config
  
  # ... 后续原有代码不变
}
```

---

### 阶段三：更新配置模板

#### 步骤 3.1：更新 userdata.txt.example

```json
{
  "cloudflare": {
    "email": "YOUR_CF_EMAIL",
    "api_token": "YOUR_CF_API_TOKEN",
    "zone_id": "YOUR_CF_ZONE_ID"
  },
  "chmlfrp": {
    "username": "YOUR_USERNAME",
    "password": "YOUR_PASSWORD",
    "token": "旧格式token（兼容用，如E3g7Q9o3RHppxJPilOGyLC5v）",
    "oauth2": {
      "enabled": true,
      "client_id": "019d534218e67f8a862056c1efb869db",
      "client_secret": "0a98ee0b7c69daa4c4922bae9be5df95eff6",
      "access_token": "",
      "refresh_token": "",
      "token_expires_at": 0
    }
  }
}
```

---

## 五、错误处理

### 5.1 错误场景与处理

| 场景 | 检测方法 | 处理方式 |
|------|----------|----------|
| 无 OAuth2 配置 | `oauth2.enabled` != true | 回退到旧 token |
| 无 client_id/secret | 字段为空 | 报错并提示配置 |
| access_token 为空 | 字段为空或 null | 尝试 refresh |
| access_token 过期 | `expires_at < now + 60` | 刷新 token |
| refresh_token 过期 | refresh 返回 invalid_grant | 重新设备码授权 |
| 设备码授权超时 | 5分钟内未授权 | 提示重试 |
| 网络错误 | curl 失败 | 指数退避重试 |

### 5.2 用户引导信息

#### 场景1：首次使用 OAuth2

```
========================================
ChmlFrp OAuth2 授权引导
========================================

检测到您启用了 OAuth2 认证，但尚未完成首次授权。

请按以下步骤操作：

1. 在浏览器打开：
   https://account-api.qzhua.net/oauth-device-verify

2. 输入用户代码：
   XXXX-XXXX

3. 使用您的 QZhua 账号登录并授权

授权成功后，脚本将自动保存 token 并继续执行。

提示：授权有效期为 5 分钟，请尽快完成。
========================================
```

#### 场景2：Token 刷新失败

```
========================================
ChmlFrp Token 刷新失败
========================================

尝试刷新 access_token 失败，可能原因：
- refresh_token 已过期
- 网络连接问题

解决方案：
1. 重新运行设备码授权流程
2. 或手动在 QZhua 控制台生成新 token

提示：如果问题持续，请检查 client_id 和 client_secret 是否正确。
========================================
```

---

## 六、测试计划

### 6.1 测试用例

| 测试编号 | 场景 | 预期结果 |
|----------|------|----------|
| T1 | 使用有效 OAuth2 token | API 正常调用 |
| T2 | token 过期后自动刷新 | 成功获取新 token |
| T3 | refresh_token 也过期 | 触发重新授权流程 |
| T4 | 未配置 OAuth2，回退到旧 token | 正常调用 API |
| T5 | 首次授权完整流程 | 成功保存 token |
| T6 | 设备码授权超时 | 提示超时并退出 |

### 6.2 手动测试命令

```bash
# 测试 Token 获取
bash chmlfrp.sh userinfo

# 测试节点列表
bash chmlfrp.sh nodes

# 测试 OAuth2 刷新（强制刷新）
# 临时将 token_expires_at 改为过去时间

# 测试故障时的回退
# 注释掉 oauth2 配置，使用旧 token
```

---

## 七、向后兼容性

### 7.1 兼容策略

1. **配置优先级**：`oauth2.enabled = true` 时完全使用 OAuth2，否则使用旧 token
2. **字段兼容**：旧配置文件的 `chmlfrp.token` 字段继续有效
3. **渐进迁移**：用户可以逐步迁移到 OAuth2，不需要一次性完成

### 7.2 迁移路径

```
旧模式                          新模式
──────────────────────────────►─────────────────────────────
│                                                          │
▼                                                          ▼
userdata.txt                    userdata.txt
{                                {
  "chmlfrp": {                    "chmlfrp": {
    "token": "旧token"    →        "oauth2": {
  }                                   "enabled": true,
}                                      "access_token": "...",
                                        "refresh_token": "...",
                                        ...
                                      },
                                      "token": "旧token(可选)"  
                                    }
                                  }
```

---

## 八、文件变更清单

| 文件 | 操作 | 变更内容 |
|------|------|----------|
| `chmlfrp.sh` | 修改 | 添加 OAuth2 管理函数，修改 token 获取逻辑 |
| `new_fix_flow.sh` | 修改 | 添加 OAuth2 获取逻辑，修改 API 调用函数 |
| `userdata.txt.example` | 修改 | 添加 OAuth2 配置字段示例 |
| `userdata.txt` | 不变 | 运行时文件，用户手动更新或脚本自动更新 |

---

## 九、风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| OAuth2 服务不可用 | 无法获取 token | 回退到旧 token 模式 |
| refresh_token 也过期 | 无法自动恢复 | 提示用户重新授权 |
| 授权超时 | 流程中断 | 增加超时提示和重试指引 |
| 配置错误 | API 调用失败 | 详细的错误提示 |
| Token 写入失败 | Token 无法保存 | 使用临时文件 + 原子移动 |

---

## 十、附录

### A. 相关文档链接

- QZhua OAuth2 设备码授权文档：https://account.qzhua.net/...
- ChmlFrp API 文档：https://docs.chmlfrp.net/API/v2/

### B. OAuth2 API 端点

| 端点 | URL |
|------|-----|
| 设备码获取 | `https://account-api.qzhua.net/oauth2/device_authorization` |
| Token 交换 | `https://account-api.qzhua.net/oauth2/token` |
| 设备验证 | `https://account-api.qzhua.net/oauth-device-verify` |

### C. Token 响应示例

```json
{
  "access_token": "eyJraWQiOiI3MzBiZGRmNC0zMjBjLTQ1MTItYjgxMy03OWM5ODE3NTVhMGYi...",
  "refresh_token": "KA3DLo9nxPBfZWNNuMB7gaP6jmsDVV9XD01q2WyFB6yBO-UBaJ57-olimMJo1gbhWwlSEHlzNF7hfL-xcIBPi_EmzSjhCA__D8cih-0u97S-wJiMU4vm3Pbox2K4YU8z",
  "scope": "chmlfrp_api",
  "token_type": "Bearer",
  "expires_in": 599
}
```
