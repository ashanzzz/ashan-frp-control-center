# 技术栈选型

## 1. 最终推荐

### Frontend: Dioxus 0.7 Web/WASM

选择 Dioxus 而不是 Vue/React，也暂不选择 Leptos。

理由：

1. 前端可以保持 100% Rust 业务组件代码；
2. Dioxus 0.7 Fullstack 直接建立在 Axum 上；
3. 支持 Server Functions、Typed Routing、SSR/Hydration、Streams、SSE/WebSocket 场景；
4. 控制台大量共享类型可以与后端共用 `api-types`；
5. 当前项目属于交互式运维控制台，Dioxus 的组件模型和 Fullstack DX 更适合快速构建复杂状态 UI；
6. 即使未来需要桌面管理端，Dioxus 仍保留扩展路径，但当前只构建 Web。

不使用 Node.js 作为生产运行时。构建阶段可以由 Dioxus CLI 完成，最终产物由 Rust Server 服务。

### Backend: Axum 0.8

理由：

- 与 Dioxus 0.7 Fullstack 官方集成；
- Tokio/Tower 生态成熟；
- 适合 REST + SSE + middleware；
- 可在 Dioxus Router 上追加稳定 `/api/v1` 路由。

### Async Runtime: Tokio 1.x

用于：

- HTTP server
- Provider 请求
- FRPC 子进程
- stdout/stderr 流
- 后台 Job
- 定时任务
- channel/broadcast

### Database: SQLite + SQLx 0.9

理由：

- 单用户自托管产品不需要 PostgreSQL；
- SQLx 让 schema 与 migration 更显式，适合全局 Failover 事务与审计；
- migrations 可以嵌入二进制；
- WAL 模式 + 合理连接池足够。

### HTTP Client: Reqwest 0.13

用于 ChmlFrp、Cloudflare 等 Provider API。

### Serialization: Serde 1.x

共享 API DTO、Provider Payload、事件模型。

### Observability: tracing

所有关键操作使用结构化 span/event：

- request_id
- job_id
- tunnel_id
- active_node_id
- target_node_id
- provider
- fault_domain
- outcome

### FRPC Process: tokio::process

FRPC 运行在同一容器内，Control Center 持有 Child handle，并读取 stdout/stderr。

### Authentication（计划项）

目标为单管理员模型；**v0.1.1 尚未实现该层，当前仅允许部署在可信网络或外部访问控制之后。**

最终模型：

- Password: Argon2id
- Session: 服务端 opaque session，不使用 JWT
- Cookie: HttpOnly + SameSite=Strict；是否 Secure 取决于部署是否 HTTPS
- Session token 只在客户端 cookie 中出现，数据库存 hash

### Secret Storage

- AES-256-GCM
- 根密钥来自 `APP_MASTER_KEY`
- 每条 secret 使用随机 nonce
- ChmlFrp / Cloudflare credential 明文不进入日志

## 2. Dioxus vs Leptos

Leptos 也是合格方案，并有官方 Axum 集成；但本项目最终选择 Dioxus 0.7，主要不是性能差异，而是开发模型：

| 维度 | Dioxus 0.7 | Leptos | 本项目选择 |
|---|---|---|---|
| Rust Web/WASM | 强 | 强 | 均可 |
| Axum 官方整合 | 强 | 强 | 均可 |
| Fullstack Server Functions | 强 | 强 | 均可 |
| 实时 Streams/WebSocket DX | 很适合 | 可实现 | Dioxus |
| 控制台组件开发体验 | React-like，直观 | Fine-grained reactive | Dioxus |
| 跨平台扩展 | 更强 | Web 为主 | Dioxus 有余量 |

因此：**Dioxus 0.7 + Axum**。

## 3. CSS / UI 方案

不引入 Vue/React/Tailwind 作为运行依赖。

第一版使用：

- 自建 CSS variables design tokens
- 原生 CSS Grid/Flex
- Dioxus 组件封装 Table/Card/Badge/Dialog/Toast/LogViewer
- SVG 图标作为静态资产

目标是减少 JS/npm 生态依赖，并让最终生产运行保持纯 Rust Server + WASM/static assets。

## 4. API 方案

同时保留两种：

- Dioxus Server Functions：仅用于内部 UI 便利调用
- `/api/v1/*`：稳定的外部/自动化 REST 契约

实时使用 `/api/v1/events/stream` SSE。

## 5. Job 方案

不引入 Redis/Celery 类外部队列。

SQLite 表：

- jobs
- job_steps
- job_events

后台 Tokio worker 扫描 queued/retry_wait Job。

Global Failover 使用数据库 lease + 进程级 mutex 双保险，确保同一时刻最多一个全局切换。

## 6. Docker 方案

多阶段构建：

1. Rust/Dioxus Builder 构建 server + WASM/static assets；
2. 下载/打包受支持的官方 frpc（或运行期受控安装，具体在实现阶段固定）；
3. 最终镜像只包含必要运行文件。

生产：

- 单容器
- 单 HTTP 端口 `8080`
- `/data` 持久化
- SQLite `/data/state.db`
- FRPC 配置 Revision `/data/frpc/config/`
- FRPC logs `/data/frpc/logs/`
- 不挂 `/var/run/docker.sock`
