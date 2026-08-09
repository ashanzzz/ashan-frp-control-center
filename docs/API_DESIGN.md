# API 设计草案

## 1. 原则

- 稳定前缀 `/api/v1`
- 所有写操作返回 Job ID 或最终结果
- 全局节点切换没有 tunnel_id 参数
- 不提供 per-tunnel failover endpoint

## 2. 主控台

- `GET /api/v1/control-center`
  - 返回全局 Routing、Provider、FRPC 状态与 Tunnel rows

- `GET /api/v1/tunnels`
- `POST /api/v1/tunnels`
- `PUT /api/v1/tunnels/{id}`
- `DELETE /api/v1/tunnels/{id}`

## 3. ChmlFrp

- `GET /api/v1/chmlfrp/nodes`
- `GET /api/v1/chmlfrp/tunnels`
- `POST /api/v1/chmlfrp/reconcile`
- `POST /api/v1/chmlfrp/refresh-config`

## 4. 全局路由

- `GET /api/v1/routing`
- `POST /api/v1/routing/switch`
  - body: `{ "target_node_id": "...", "reason": "manual" }`
  - 这是全局切换。

- `POST /api/v1/routing/failover/enable`
- `POST /api/v1/routing/failover/disable`

明确禁止：

```text
POST /tunnels/{id}/failover
```

此 API 不存在。

## 5. FRPC

- `GET /api/v1/frpc/runtime`
- `POST /api/v1/frpc/start`
- `POST /api/v1/frpc/stop`
- `POST /api/v1/frpc/restart`
- `POST /api/v1/frpc/refresh-config`
- `GET /api/v1/frpc/events`

## 6. DNS

- `GET /api/v1/dns/managed-records`
- `GET /api/v1/dns/all-records`（只读）
- `POST /api/v1/dns/reconcile`

## 7. Jobs / Activity

- `GET /api/v1/jobs`
- `GET /api/v1/jobs/{id}`
- `GET /api/v1/activity`

## 8. SSE

- `GET /api/v1/events/stream`

事件类型：

- `frpc.log`
- `frpc.state`
- `tunnel.state`
- `routing.state`
- `failover.started`
- `failover.step`
- `failover.completed`
- `failover.failed`
- `dns.state`
- `job.state`
