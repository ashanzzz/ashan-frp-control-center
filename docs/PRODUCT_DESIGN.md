# 产品设计

## 1. 产品定位

Ashan FRP Control Center 是一个个人自托管的 ChmlFrp 全局隧道控制平面。产品以“计划隧道清单”为唯一事实来源，将 ChmlFrp、FRPC、Cloudflare 三个执行/观测系统统一到一个控制台中。

它解决的核心问题不是“怎么创建一条 FRP 隧道”，而是：

> 如何保证全部受管隧道始终统一运行在同一个健康 ChmlFrp 节点上，并在任一明确的服务端节点故障发生时自动整体迁移。

## 2. 核心用户心智

用户只维护：

- 隧道名
- 本地 IP
- 本地端口
- 协议
- 公网域名
- 是否管理 Cloudflare DNS

用户不为每条隧道选择节点。节点是全局资源。

## 3. 全局节点模型

系统只有：

- 一个 `ACTIVE` 节点
- 一个 `STANDBY` 节点
- 0..N 个 `AVAILABLE` 候选节点
- 0..N 个 `QUARANTINED` 隔离节点
- 0..N 个 `DISABLED` 人工禁用节点

所有计划隧道统一绑定当前 `ACTIVE` 节点。

## 4. 隧道主控台

### 4.1 顶部全局区

只显示一次节点信息，不在每一行重复：

- Active Node：名称 / Node ID / 公网 IP / 状态
- Standby Node：名称 / Node ID / 公网 IP / 状态
- FRPC：Running/Stopped、Connected/Disconnected、最近关键错误
- ChmlFrp API：Connected/Failed
- Cloudflare API：Connected/Failed
- 自动 Failover：Enabled/Disabled
- 隔离时间：默认 30 天
- 当前 Job：无 / 部署中 / 全局切换中

### 4.2 隧道表

字段固定为：

| 字段 | 含义 |
|---|---|
| 隧道 | 计划隧道名称 |
| 本地地址 | `IP:Port` |
| 域名 | 计划公网域名 |
| ChmlFrp | 是否存在 / 是否一致 |
| FRPC | 从实时日志解析出的运行状态 |
| Cloudflare | A 记录是否存在 / IP 是否一致 |
| 整体状态 | 三层状态合成结果 |

不显示每条隧道的主节点、备用节点、当前节点，因为这些永远是全局统一状态。

## 5. 三层状态语义

### ChmlFrp

- `EXISTS`：已存在且与计划一致
- `DRIFTED`：已存在但配置不一致
- `MISSING`：不存在
- `CREATING` / `UPDATING`：同步中
- `DISABLED`：远端已禁用

### FRPC

FRPC 不负责配置所有权，只负责运行 ChmlFrp 生成的配置并提供日志事实。

Tunnel-level：

- `HEALTHY`
- `STARTING`
- `NOT_LOADED`
- `LOCAL_SERVICE_ERROR`
- `CONFIG_ERROR`
- `REMOTE_PROXY_CONFLICT`
- `REMOTE_ERROR`

Global/runtime-level：

- `RUNNING_CONNECTED`
- `RUNNING_DISCONNECTED`
- `STOPPED`
- `NODE_FAILURE`

### Cloudflare

- `EXISTS_MATCHED`
- `EXISTS_IP_MISMATCH`
- `MISSING`
- `UPDATING`
- `UNMANAGED`

## 6. 整体状态

示例：

- ChmlFrp 已存在 + FRPC 正常 + Cloudflare 匹配 -> `HEALTHY`
- ChmlFrp 不存在 -> `NOT_DEPLOYED`
- FRPC 本地服务错误 -> `LOCAL_SERVICE_ERROR`
- FRPC 明确节点故障 -> 所有行统一进入 `GLOBAL_FAILOVER`
- Cloudflare IP 不一致 -> `DNS_DRIFT`

## 7. 页面结构

一级导航：

1. 总览
2. 隧道主控台
3. ChmlFrp
4. DNS
5. FRPC
6. 自动化
7. 活动
8. 设置

### ChmlFrp 页面

只管理全局节点池与查看实际隧道：

- Active / Standby 顶部卡片
- 可用节点池
- 隔离节点及剩余隔离时间
- 实际 ChmlFrp 隧道清单
- 节点切换历史

业务字段修改回到“隧道主控台”，避免双重事实来源。

### DNS 页面

- Cloudflare 连接与 Zone
- 当前全局目标 IP
- 受管 A 记录列表
- 实际 IP vs 目标 IP
- 非受管记录只读

### FRPC 页面

只提供：

- FRPC 进程状态
- 服务端连接状态
- 当前加载配置来源/时间/Revision
- 实时 stdout/stderr 日志
- 日志分类结果
- Start / Stop / Restart / 重新获取 ChmlFrp 配置

不提供人工编辑 FRPC 配置。

## 8. 手动切换

人工“切换节点”也是全局切换，复用同一 Global Failover/Switchover Engine；不存在单隧道切换按钮。
