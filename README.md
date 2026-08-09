# Ashan FRP Control Center

> **Rust 全栈的 ChmlFrp 全局隧道控制中心。** 一份计划隧道清单统一控制 ChmlFrp、FRPC 与 Cloudflare；故障域是“节点”，不是单条隧道。

> [!WARNING]
> v0.1.0 面向可信内网 / Unraid WebUI 使用，当前未内置多用户认证。不要把 8080 管理端口直接暴露到公网；如需远程访问，请先放在你自己的可信反向代理/VPN/访问控制后面。


## v0.1.0 重构原则

本仓库是按产品重新设计后的 Rust 实现，不继承 Vue/Node/Go 技术栈。

### 不可违反的核心规则

1. **所有受管隧道共用一个全局活动节点。**
2. **不存在单隧道 Failover。** 任意一条 FRPC 日志只要确认是当前 ChmlFrp 服务端/节点故障，就触发一次 `GLOBAL_FAILOVER`。
3. `GLOBAL_FAILOVER` 的作用范围永远是 **ALL_MANAGED_TUNNELS**：所有计划隧道统一迁到同一个备用节点。
4. 故障节点立刻进入隔离期，默认 30 天；隔离期内绝不参与候选。
5. ChmlFrp 是隧道配置事实执行层：创建/修改隧道并生成 `frpc.ini`。
6. FRPC 不生成业务配置，只运行 ChmlFrp 生成的配置，并提供运行状态与日志故障信号。
7. Cloudflare DNS 永远最后更新：只有 ChmlFrp 全量迁移 + 新 FRPC 配置启动成功后，才统一把全部受管 A 记录改到新节点公网 IP。
8. Cloudflare 只修改在主控台登记为受管的域名。

## 隧道主控台

主列表固定显示：

| 隧道 | 本地地址 | 域名 | ChmlFrp | FRPC | Cloudflare | 整体状态 |
|---|---|---|---|---|---|---|
| new-api | 192.168.8.11:3001 | api.example.com | 已存在 | 正常 | 已存在 | 正常 |

节点信息只在顶部全局显示一次，因为所有隧道统一节点、统一切换。

## 技术栈

- 前端：Dioxus 0.7 Web/WASM（Rust）
- 后端：Axum 0.8 + Tokio
- 数据库：SQLite + SQLx 0.9
- Provider：Reqwest 0.13
- 实时通道：SSE `/api/v1/events`
- FRPC：`tokio::process` 子进程管理 + stdout/stderr 日志解析
- 部署：单 Docker、单 8080 WebUI、单 SQLite、无 Redis、无 Nginx、无 Docker Socket

Dioxus 0.7 Fullstack/工具链围绕 Axum 构建；本仓库采用“Dioxus 静态 Web/WASM + 独立 Axum API”部署方式，保持前后端都为 Rust，同时让 FRPC 生命周期和 Provider 编排边界更明确。

## 当前实现的核心能力

- 计划隧道 CRUD
- 全局 Active / Standby / 隔离天数
- ChmlFrp v2：隧道列表、创建、修改节点、节点详情、节点列表、生成配置文件
- Cloudflare：A Record 列表、创建、PATCH 更新；节点切换仅改 IP，并保留已有 Proxied 状态
- FRPC：启动/停止/重启、运行状态、实时日志、每条隧道日志状态
- FRPC 日志分类：本地服务、配置、认证、服务端冲突、服务端断连、模糊网络故障
- 明确 Node 级日志事件自动触发全局 Failover
- 全局 Failover Lock，禁止并行切换
- 故障节点 30 天（可配置）隔离
- 全部 ChmlFrp 隧道统一迁移
- ChmlFrp 新配置下载 + FRPC Restart
- Cloudflare 受管 A 记录最后统一切 IP
- Activity 审计记录
- SSE FRPC 事件流
- Dioxus 隧道主控台 / ChmlFrp / DNS / FRPC / Activity 页面

## ChmlFrp API 兼容说明

实现基于 ChmlFrp 官方 v2 文档：

- `GET /tunnel`
- `POST /create_tunnel`
- `POST /update_tunnel`
- `GET|POST /tunnel_config`
- `GET|POST /node`
- `GET|POST /nodeinfo`

官方当前文档仍标注 `/update_tunnel` 对 HTTP/HTTPS 类型存在限制。Ashan FRP Control Center **不会绕过 Provider 返回的错误**：如果 ChmlFrp 拒绝某条网站隧道迁移，全局 Failover Job 会失败并停止在 DNS 更新之前，避免形成“DNS 已切但新链路没准备好”的危险状态。可根据你的实际 ChmlFrp 账号/API行为继续增加兼容策略。

参考：

- https://docs.chmlfrp.net/API/v2/Tunnel_operations/update_tunnel.html
- https://docs.chmlfrp.net/API/v2/Tunnel_operations/tunnel_config.html
- https://docs.chmlfrp.net/API/v2/Node_operations/nodeinfo.html
- https://developers.cloudflare.com/api/resources/dns/subresources/records/

## 本地 / Unraid 部署

### 1. 准备数据目录

```bash
mkdir -p data/frpc
```

将你实际使用的 **ChmlFrp FRPC 客户端二进制** 放到：

```text
data/frpc/frpc
```

Linux 下：

```bash
chmod +x data/frpc/frpc
```

程序不会自己杜撰 FRPC 配置；每次全局同步/切换后都从 ChmlFrp `/tunnel_config` 获取完整配置并保存为 `/data/frpc/frpc.ini`。

### 2. 配置

```bash
cp .env.example .env
```

至少填写：

```env
CHMLFRP_TOKEN=...
CLOUDFLARE_API_TOKEN=...
CLOUDFLARE_ZONE_ID=...
ACTIVE_NODE=你的主节点名称
STANDBY_NODE=你的备用节点名称
QUARANTINE_DAYS=30
```

### 3. 启动

```bash
docker compose up -d --build
```

WebUI：

```text
http://UNRAID-IP:8080
```

### 4. 第一批隧道

在“隧道主控台”录入计划：隧道名、本地 IP、本地端口、协议、域名。**不要给每条隧道选择节点。** 然后点击“全局同步”。

## 全局故障切换

```text
任意 FRPC 日志
      ↓
FrpcLogClassifier
      ↓
确认 fault_domain = NODE
      ↓
GLOBAL_FAILOVER LOCK
      ↓
旧 Active Node → QUARANTINED
      ↓
选择唯一 Target Node
      ↓
ChmlFrp：全部计划隧道一起迁移
      ↓
/tunnel_config 获取目标节点完整配置
      ↓
Restart FRPC
      ↓
FRPC 运行验证
      ↓
Cloudflare：全部受管 A 记录 → 新节点 realIp
      ↓
提交新的全局 Active Node
```

`already exist` / proxy 冲突：如果下载的本地配置预检查没有重复 Proxy Name，则按服务端冲突归类为 Node Fault，可触发全局切换；如果配置本身有重复，则归类为配置错误，不切节点。

## API

- `GET /api/v1/health`
- `GET /api/v1/dashboard`
- `GET|POST /api/v1/tunnels`
- `PUT|DELETE /api/v1/tunnels/{id}`
- `GET|PUT /api/v1/routing`
- `GET /api/v1/nodes`
- `POST /api/v1/reconcile`
- `POST /api/v1/failover`
- `GET /api/v1/frpc/status`
- `GET /api/v1/frpc/logs`
- `POST /api/v1/frpc/start|stop|restart`
- `GET /api/v1/events` (SSE)

## 校验

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

dx build --release --platform web
cargo build --release -p ashan-frp-server
```

当前生成环境无法联网安装 Rust toolchain，因此 ZIP 生成前执行的是仓库结构、TOML/YAML/SQL、Git diff 与静态一致性检查；在有 Rust 的机器或 GitHub Actions 中应执行上述完整编译测试。

## 文档

完整产品约束位于 `docs/`，尤其：

- `docs/PRODUCT_DESIGN.md`
- `docs/FAILOVER_DESIGN.md`
- `docs/ARCHITECTURE.md`
- `docs/DATA_MODEL.md`
- `docs/TECH_STACK.md`
- `docs/ADR/0002-global-node-failover.md`
