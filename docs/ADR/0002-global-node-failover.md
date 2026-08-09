# ADR-0002: All tunnels use one global node and fail over together

Status: Accepted

## Decision

所有受管隧道始终统一使用一个全局 Active Node。任意一条 FRPC 日志确认当前服务端节点故障时，触发一个 Global Failover Job，将所有受管隧道整体迁移到同一备用节点。

## Explicitly rejected

- per-tunnel active node
- per-tunnel standby node
- per-tunnel failover endpoint
- 在不同节点之间混合运行作为正常最终状态

## Failover order

1. Fence + quarantine old node
2. Select target node
3. ChmlFrp bulk migration for all managed tunnels
4. Fetch ChmlFrp-generated FRPC config
5. Restart FRPC
6. Verify via FRPC logs
7. Bulk-update all managed Cloudflare A records
8. Commit new RoutingState
