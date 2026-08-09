# 数据模型设计

## 1. 核心原则

数据库中**绝不保存每条隧道独立的 active_node_id / standby_node_id**。

节点选择只存在于全局 `routing_state`。

## 2. tunnel_plans

计划隧道，主控台唯一事实来源。

建议字段：

- id
- name UNIQUE
- display_name
- local_ip
- local_port
- protocol
- domain
- dns_managed
- enabled
- created_at
- updated_at

不包含 per-tunnel node assignment。

## 3. nodes

ChmlFrp 节点缓存/本地策略：

- id
- provider_node_id UNIQUE
- name
- public_ip
- provider_status
- supports_web
- priority
- local_state: AVAILABLE/STANDBY/ACTIVE/QUARANTINED/DISABLED
- quarantine_until
- quarantine_reason
- last_failure_at
- last_validated_at

## 4. routing_state

单行表：

- singleton_id = 1
- active_node_id
- standby_node_id
- state: ACTIVE/FAILOVER/DEGRADED
- failover_enabled
- quarantine_days
- revision
- updated_at

所有 Tunnel 的目标节点由此表推导。

## 5. chmlfrp_tunnel_state

每条计划隧道的远端事实：

- tunnel_plan_id
- remote_tunnel_id
- exists
- config_hash
- drift_status
- last_synced_at
- last_error

这里只记录事实，不记录独立节点策略。

## 6. frpc_runtime

全局一行：

- pid
- status
- server_connection_status
- config_revision
- started_at
- last_exit_code
- last_error

## 7. frpc_tunnel_state

每条 Tunnel 的日志投影：

- tunnel_plan_id
- runtime_status
- last_event_type
- last_event_at
- last_error_class
- last_error_excerpt_safe

## 8. frpc_events

结构化日志事件：

- id
- timestamp
- tunnel_plan_id nullable
- proxy_name nullable
- event_type
- fault_domain
- severity
- triggers_failover
- raw_excerpt_safe
- failover_job_id nullable

## 9. dns_bindings

系统受管 DNS：

- tunnel_plan_id
- provider = cloudflare
- zone_id
- record_id
- hostname
- record_type = A
- managed = true
- actual_ip
- expected_ip
- last_synced_at

expected_ip 永远由 `routing_state.active_node_id -> nodes.public_ip` 推导。

## 10. jobs / job_steps

所有外部写操作必须可审计：

- DEPLOY_ALL
- RECONCILE
- GLOBAL_FAILOVER
- MANUAL_GLOBAL_SWITCH
- DNS_SYNC
- FRPC_RESTART

Global Failover 是单 Job、多 Step，不是 N 个 per-tunnel failover jobs。
