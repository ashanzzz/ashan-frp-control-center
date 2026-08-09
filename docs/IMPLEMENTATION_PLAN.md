# 重构实施计划

## Phase 0 - Design freeze

先固定以下约束并写入测试：

- 不存在 per-tunnel failover；
- 所有 Tunnel 目标节点由全局 RoutingState 推导；
- FRPC 配置只能来自 ChmlFrp；
- Cloudflare 永远最后切；
- 故障节点必须先隔离再选择目标；
- 明确 Node Fault 一条即可触发全局切换。

## Phase 1 - Rust skeleton

- Cargo Workspace
- Dioxus 0.7 Fullstack
- Axum Router
- SQLx + SQLite migrations
- Auth
- tracing
- basic UI shell

## Phase 2 - Control Center domain

- tunnel_plans
- node pool
- routing_state
- 主控台 GET API
- Dioxus Tunnel Table

## Phase 3 - ChmlFrp adapter

- credentials
- node discovery
- tunnel inventory
- reconcile
- bulk node migration
- generated FRPC config retrieval

## Phase 4 - FRPC runtime + log intelligence

- process supervisor
- config revision storage
- stdout/stderr stream
- parser
- per-tunnel runtime projection
- node fault classifier

此阶段必须建立真实日志 fixture 测试，避免用过宽正则导致误切换。

## Phase 5 - Global Failover Engine

- global lock
- quarantine
- target selection
- ChmlFrp bulk migration
- config fetch
- FRPC restart
- log verification
- forward recovery

## Phase 6 - Cloudflare

- managed A bindings
- drift read
- bulk A update after FRPC success
- final verification

## Phase 7 - Jobs / Activity / Realtime

- persistent jobs
- job steps
- SSE
- Failover progress UI
- audit

## Phase 8 - Docker / Unraid

- single production image
- internal port 8080
- /data persistence
- healthcheck
- GHCR workflow
- Unraid template
