# 全局故障切换设计

## 1. 核心规则

系统只实现 **Global Node Failover**，不实现 Per-Tunnel Failover。

任意一条隧道的 FRPC 日志，只要被确认是当前 ChmlFrp 服务端/节点导致的故障，即把故障域提升到 `NODE`，触发全部受管隧道整体迁移。

## 2. FRPC 日志是主要故障事实来源

FRPC 日志解析器输出标准事件：

```text
FrpcEvent {
  timestamp
  proxy_name?       // 若日志可关联到具体隧道
  event_type
  fault_domain      // LOCAL / CONFIG / AUTH / NETWORK / NODE
  severity
  raw_excerpt_safe
  triggers_failover
}
```

## 3. 错误分类

### 3.1 明确节点/服务端故障：立即触发全局切换

示例语义：

- 远端服务端主动关闭/重置控制连接
- 服务端 Session 异常退出
- 远端节点明确拒绝 Proxy，且已排除本地配置重复
- 服务端残留导致的 Proxy Name Conflict，且本地配置唯一性检查通过
- 已建立连接后出现可明确归因于当前节点的远端连接故障

统一输出：

```text
fault_domain = NODE
triggers_failover = true
```

只需要一条明确事件，不要求多条隧道同时失败。

### 3.2 本地服务故障：不切节点

例如本地 `IP:Port` connection refused。

```text
fault_domain = LOCAL
triggers_failover = false
```

### 3.3 本地配置错误：不切节点

例如 ChmlFrp 下载配置本身包含重复 Proxy Name。

必须先做配置唯一性预检查。若本地确实重复，则归类 `CONFIG`，不能误判成节点故障。

### 3.4 认证/Token 错误：不切节点

归类 `AUTH`，直接告警并阻断自动切换。

### 3.5 模糊网络错误

单纯 timeout 不一定能证明是 ChmlFrp 节点故障，因此进入快速确认流程：

1. 检查本机基础网络是否正常；
2. 对当前 ChmlFrp 节点进行短窗口复核；
3. 只有故障可归因于当前节点时，升级为 `NODE` 并全局切换。

## 4. 节点状态机

```text
AVAILABLE
   |
   v
ACTIVE
   |
   | confirmed NODE fault
   v
FAILED
   |
   v
QUARANTINED
   |
   | quarantine expires
   v
PROBATION
   |
   | validation success
   v
AVAILABLE
```

另有 `STANDBY` 和 `DISABLED`。

默认隔离时间 30 天，可在设置中修改。

隔离期内节点绝不参与 Active/Standby 选择。

隔离到期后仅恢复候选资格，不自动切回原节点。

## 5. 全局切换事务

一次切换必须只有一个全局 Job：

```text
GLOBAL_FAILOVER_JOB
```

禁止拆成每条隧道独立切换 Job。

### Phase 0 - Lock

获取全局 Failover Lock，阻止并行部署、第二次切换和手工同步修改路由状态。

### Phase 1 - Fence old node

- 当前 Active -> FAILED
- 写入失败原因与触发日志事件
- 立即写入 `quarantine_until = now + configured_days`

### Phase 2 - Select target

从可用节点池选择 Standby/最高优先级候选：

- 非当前故障节点
- 非隔离节点
- 非禁用节点
- 支持网站/目标能力
- ChmlFrp 状态可用

### Phase 3 - ChmlFrp bulk migration

对**全部计划隧道**执行统一节点迁移。

要求：

- 目标节点必须完全一致；
- 任何一条失败，都不能把 Job 标成成功；
- 不允许形成 A/B 节点混合状态作为最终态。

### Phase 4 - Obtain generated FRPC config

ChmlFrp 完成迁移后：

- 从 ChmlFrp 获取它生成的新 FRPC 配置；
- 保存为新的不可变 Revision；
- 做基本语法/Proxy Name 唯一性预检查；
- 不由 Control Center 自行重新生成业务配置。

### Phase 5 - Restart FRPC

使用新的 ChmlFrp 配置启动/重启 FRPC。

### Phase 6 - FRPC log verification

以 FRPC 实时日志验证：

- 成功连接新服务端；
- 所有计划 Proxy 均达到成功运行状态；
- 不存在新的节点级错误。

如果目标节点失败，进入 Forward Recovery：把该目标节点也隔离，重新选择下一个候选节点；不要回滚到已判定故障的旧 Active。

### Phase 7 - Cloudflare bulk A update

只有 Phase 6 成功后才修改 DNS。

将全部本系统受管 A 记录统一从旧节点 IP 更新到新 Active Node IP。

不修改：

- 未登记为本系统受管的记录；
- MX/TXT/CAA 等无关记录；
- 其他人工维护域名。

### Phase 8 - Verify and commit

验证：

- ChmlFrp 全部计划隧道都在新节点；
- FRPC 运行且日志正常；
- 所有受管 A 记录实际 IP == 新 Active Node IP。

最后提交全局路由状态：

```text
active_node_id = target
standby_node_id = next_candidate_or_null
state = ACTIVE
```

## 6. UI 规则

切换过程中，主控台所有隧道行统一显示 `GLOBAL_FAILOVER`，不显示“某隧道正在单独切换”。

触发源可记录为：

```text
Triggered by: openclaw
Reason: SERVER_NODE_FAILURE
```

但作用范围始终为 `ALL_MANAGED_TUNNELS`。
