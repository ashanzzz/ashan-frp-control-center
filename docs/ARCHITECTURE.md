# Rust 系统架构

## 1. 架构目标

- Rust 前端 + Rust 后端
- 单容器、单端口
- SQLite 单机持久化
- 无 Redis、无 PostgreSQL、无 Nginx、无独立前端容器
- FRPC 作为本容器内受控子进程运行
- 不挂 Docker Socket
- ChmlFrp 是隧道配置与 FRPC 配置来源
- Cloudflare 是 DNS 执行目标
- FRPC 是运行时与故障观测源

## 2. 推荐 Workspace

```text
apps/control-center
crates/domain
crates/database
crates/auth        # planned hardening; not implemented in v0.1.1
crates/chmlfrp
crates/cloudflare
crates/frpc-runtime
crates/frpc-log
crates/failover
crates/jobs
crates/observability
crates/api-types
```

## 3. 模块职责

### domain

纯领域模型与状态机，不依赖 Axum、Dioxus、SQLx。

包含：

- TunnelPlan
- RoutingState
- Node
- NodeState
- FrpcEvent
- TunnelRuntimeState
- DnsState
- FailoverState
- Job / JobStep

### database

SQLx repository、transaction、migration。

禁止在 Handler 中直接写 SQL。

### chmlfrp

ChmlFrp API Adapter：

- 获取节点
- 获取/创建/修改/删除隧道
- 批量切换全部计划隧道
- 获取 ChmlFrp 生成的 FRPC 配置
- Provider 错误归一化

### cloudflare

Cloudflare Adapter：

- Zone/连接验证
- 读取受管 A 记录
- 创建/修复受管 A 记录
- 全局 Failover 时批量切换到新节点 IP

### frpc-runtime

只管理进程：

- Start
- Stop
- Restart
- PID / uptime
- stdout/stderr pipe
- 当前配置 Revision

使用 `tokio::process::Command`。

### frpc-log

负责：

- 流式读取 stdout/stderr
- 日志格式解析
- Proxy Name 关联
- Fault Domain 分类
- Tunnel-level 状态投影
- Node-level 故障事件输出

### failover

唯一允许修改全局 `RoutingState.active_node` 的业务模块。

所有自动/手动切换都必须经过同一状态机。

### jobs

SQLite 持久化 Job + Step。

部署、同步、切换都是可恢复、可审计的 Job。

### observability

`tracing`、request_id、job_id、safe error、JSONL/数据库审计。

## 4. 前后端边界

Dioxus 页面不直接调用 Provider。

```text
Dioxus UI
   |
   +--> Server Functions（内部 UI 便利调用）
   |
   +--> /api/v1/*（稳定 REST）
             |
             v
          Services
             |
             v
     Domain + Repositories
             |
      +------+------+------+
      |             |      |
   ChmlFrp       FRPC   Cloudflare
```

保留稳定 `/api/v1`，避免前端框架与外部自动化 API 强耦合。

## 5. 实时数据

优先使用 SSE：

- FRPC 实时日志
- Job Step
- Failover 状态
- 隧道状态变化

用户写操作继续用普通 HTTP/Server Function。

本产品绝大多数实时流量是 Server -> Browser，因此没有必要默认上 WebSocket。
