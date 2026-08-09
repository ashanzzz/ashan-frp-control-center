# UI 设计

## 1. 隧道主控台是核心页面

### 顶部 Global Routing Bar

```text
ACTIVE NODE
成都 A / Node 123 / 1.2.3.4 / ACTIVE

STANDBY NODE
广州 B / Node 456 / 5.6.7.8 / STANDBY

FRPC
Running / Connected

ChmlFrp
Connected

Cloudflare
Connected

Failover
Enabled / Quarantine 30d
```

发生切换时顶部整块变成 Failover Progress：

```text
GLOBAL FAILOVER
成都 A -> 广州 B
Triggered by: openclaw
Reason: SERVER_NODE_FAILURE
Step: 4 / 8 - Restarting FRPC
```

### Tunnel Table

```text
隧道 | 本地地址 | 域名 | ChmlFrp | FRPC | Cloudflare | 状态
```

禁止出现每行节点列。

FRPC 单元格可点击打开最近该 Proxy 的结构化日志，但不会出现“切换此隧道”按钮。

## 2. 切换期间表格行为

当全局 Failover 开始：

- 所有行整体状态统一显示“全局切换中”；
- ChmlFrp 列按批量迁移事实刷新；
- FRPC 列进入等待新配置/启动/验证；
- Cloudflare 在 FRPC 验证成功前显示“等待切换”；
- DNS 更新后逐行变为“已存在”。

## 3. ChmlFrp 页面

顶部只显示 Active / Standby。

节点池表：

- 节点名
- Node ID
- IP
- Provider 状态
- 本地角色
- 优先级
- 隔离剩余
- 最近故障

所有 Tunnel 仍统一使用 Active Node。

## 4. FRPC 页面

上半区：

- Running / Stopped
- Connected / Disconnected
- PID
- Uptime
- ChmlFrp Config Revision
- Config fetched at

下半区：实时日志终端。

支持过滤：

- ALL
- INFO
- WARN
- ERROR
- Tunnel Name
- NODE FAULT
- CONFIG
- LOCAL SERVICE

## 5. DNS 页面

顶部：Cloudflare connection、Zone、当前全局目标 IP。

表：

```text
域名 | 隧道 | 当前 A | 目标 A | 状态
```

目标 A 统一来自当前 Active Node IP。
