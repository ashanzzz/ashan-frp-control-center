# ChmlFrp 自动切换与隧道同步脚本

这个项目的目标很直接：

1. 在 Unraid 上持续监控 `frpc`
2. 节点掉线时自动切线
3. 固定隧道配置有变更时自动同步
4. 自动同步 Cloudflare DNS
5. 让后续的人或 AI 接手时，不需要重新猜这套脚本在干什么

这次整理的重点不是“多加几个功能”，而是把几个一直容易误解的点讲清楚：

- 哪个文件是人工输入
- 哪个文件是 API 快照缓存
- 哪个文件是运行状态
- 哪个文件只是诊断输出
- 哪些接口能用，哪些接口名义上有、实际上不能依赖

---

## 先说结论

这套系统现在由两个主脚本 + 一个共享库组成：

- `chmlfrp.sh`：控制器
- `new_fix_flow.sh`：执行器
- `chmlfrp_shared.sh`：共享状态与结果契约层

它们不是平级的两套逻辑。

正常理解方式应该是：

- `chmlfrp.sh` 负责“判断要不要动、该切到哪、什么时候动”
- `new_fix_flow.sh` 负责“真的去拉取状态、同步 DNS、创建隧道、更新 frpc 容器”
- `chmlfrp_shared.sh` 负责“统一路径、JSON 状态文件、ban / cooldown、执行结果回传”

---

## 当前已确认的 API 现实情况

### V2 API：主力接口

目前真正可依赖的是这些：

- `GET /node`
- `GET /nodeinfo?token=&node=`
- `GET /tunnel?token=`
- `POST /create_tunnel`
- `GET /tunnel_config?token=&node=`

### 删除隧道：目前不要再假设可用

这件事必须单独说。

#### V2 删除隧道

官方文档里写过 `POST /delete_tunnel`，但同时明确标注：

> 该接口还在开发中，暂时无法使用。

#### V1 删除隧道

旧文档里还保留着 `/api/deletetl.php?token=&userid=&nodeid=`，但当前实测返回的是 nginx 404。

也就是说，脚本层面现在不能再把“自动删除旧隧道”当成一个稳定能力。

这不是脚本写法问题，而是接口现实状态问题。

所以后续维护时要记住一件事：

> “拉取隧道列表”是可靠能力；“删除隧道”目前不是。

---

## 项目目录里的文件，应该怎么理解

现在统一采用一个原则：

> 运行态文件全部使用 `.txt` 扩展名，但文件内容是 JSON。

这样做的原因很简单：

- 保留你原来习惯的 `.txt` 文件管理方式
- 同时让脚本和 AI 都能稳定解析

---

## 文件分层

### 1. 人工维护的源配置

这些文件是“输入”，不是缓存。

#### `settings.env`

运行策略配置。

典型内容：

- 节点筛选条件
- 冷却时间
- ban 时间
- 是否只看支持建站的节点

#### `userdata.txt`

主配置文件，JSON 结构。

包含：

- ChmlFrp 用户名密码
- QZhua OAuth2 配置
- Cloudflare 配置

#### `fixed_tunnels.txt`

固定隧道定义。它是整个系统最重要的“期望状态”来源。

执行器会基于这里的 `name` 做字符归一，并自动去掉尾部数字后缀后作为运行时隧道名；原始 `name` 仍保留为配置源，便于人工维护。

如果你想增删隧道，正常情况下应该先改这个文件。

#### `exempt_names.txt`

豁免名单。列表中的隧道或 DNS 不参与普通清理流程。

---

### 2. 控制器生成的节点缓存 / 状态文件

这些主要由 `chmlfrp.sh` 写入。

#### `chmlfrp-nodes-all.txt`

全量节点缓存。

来源：`GET /node`

用途：

- 看官方现在到底返回了哪些节点
- 判断某个节点是“官方就没给”，还是“被本地过滤掉了”
- 给手动指定节点、当前节点反查提供全量参考

这个文件现在是排查问题的第一现场。

#### `chmlfrp-nodes-filtered.txt`

筛选后的候选节点缓存。

它不是“全量节点表”，而是：

- 基于 `chmlfrp-nodes-all.txt`
- 再套用 `settings.env` 里的过滤条件
- 最后得到的候选集合

文件内包含：

- `refreshed_at`
- `filters`
- `total_before`
- `total_after`
- `nodes`

以后再遇到“北京节点为什么没被选到”这种问题，先看这个文件，再看全量文件。

#### `chmlfrp-node-refresh-state.txt`

节点刷新状态。

用于回答：

- 上次什么时候刷新节点列表
- 刷新时用了哪个文件
- 刷新前后节点数各是多少

#### `chmlfrp-node-metrics.txt`

测速结果。

现在不再是 TSV 文本，而是 JSON 结构。

里面保存的是本次测速真正入选候选的节点，以及它们的：

- 分数
- 平均延迟
- 丢包率
- ping 目标 IP
- 用于 DNS 的 IP

#### `chmlfrp-health-status.txt`

健康检查结果。

这是 `health_check()` 的输出，不是日志。

结构里至少包含：

- `status`
- `reason`
- `details`
- `proxy_status`
- `proxy_reason`
- `ts`

其中 `status` 主要表示容器/连通性，`proxy_status` 用来单独记录代理层是否有冲突或配置问题。

#### `chmlfrp-last-switch-state.txt`

最近一次成功切换的时间。

只用于冷却判断。

#### `chmlfrp-ban-state.txt`

被 ban 的节点列表。

如果某节点切换失败，控制器会把它短期拉黑，避免同一轮反复打它。

如果执行器在容器重启后检测到“已成功登录，但多个 proxy 连续出现 `router config conflict` / `proxy already exists`”，控制器会把该节点判定为**节点级不可用**，并按更长的冷却时间写入这个文件。

#### `chmlfrp-node-issues.txt`

节点级问题记录。

用于保留：

- 哪个节点被判定为不可用
- 原因分类（例如 `node_proxy_conflict`）
- 首次记录时间
- ban 到什么时候

#### `chmlfrp-sync-result.txt`

执行器最近一次运行结果。

它是 `new_fix_flow.sh` 写给 `chmlfrp.sh` 的结果契约，用来表达：

- 这次是成功、快速退出，还是失败
- 失败属于本地配置问题、配额问题，还是节点级服务端冲突

#### `chmlfrp-userinfo.txt`

登录接口同步下来的用户详情。

主要作用：

- 给执行器读取 `userid`
- 少一次重复登录

---

### 3. 执行器生成的隧道 / DNS 快照文件

这些主要由 `new_fix_flow.sh` 写入。

#### `chmlfrp-fixed-tunnels-normalized.txt`

固定隧道配置的标准化结果。

来源：`fixed_tunnels.txt`

用途：

- 把人工输入统一成脚本内部稳定结构
- 后续 DNS 创建、隧道创建都基于它

#### `chmlfrp-tunnels-raw.txt`

ChmlFrp 当前隧道列表原始快照。

来源：`GET /tunnel?token=`

结构里保留：

- `refreshed_at`
- `source`
- `api_response`

#### `chmlfrp-tunnels-normalized.txt`

隧道列表标准化结果。

它是后续对比和清理逻辑真正使用的文件。

结构里保留：

- `refreshed_at`
- `total`
- `tunnels`

#### `cloudflare-dns-raw.txt`

Cloudflare 当前 DNS 列表原始快照。

#### `cloudflare-dns-normalized.txt`

Cloudflare DNS 标准化结果。

后续删除和创建 DNS 时，优先读这个。

#### `chmlfrp-source-snapshot.txt`

源文件快照状态。

用于记录：

- `fixed_tunnels.txt` 的 mtime/hash
- `exempt_names.txt` 的 mtime/hash

这个文件用于判断：

> 这次是否真的有配置变化，还是可以跳过一次大修复。

---

## 两个脚本分别负责什么

### `chmlfrp.sh`

这是控制器。

它的职责不是直接改资源，而是做决策。

主要入口：

- `health`
- `failover`
- `fastest`
- `manual`
- `nodes`
- `userinfo`
- `oauth_refresh`
- `oauth_reauth`

### `new_fix_flow.sh`

这是执行器。

它负责：

- 读取固定隧道定义
- 拉取当前隧道 / DNS 快照
- 选择目标节点
- 创建 DNS
- 创建隧道
- 拉取 frpc 配置
- 重启 frpc 容器

它可以单独运行，但从整体设计上看，最好还是把它当成 `chmlfrp.sh` 的下游执行器。

---

## 真实运行流程

### health

只做健康检查，写入 `chmlfrp-health-status.txt`。

它会检查：

1. Docker 容器是否存在
2. Docker 容器是否运行
3. `frpc.toml` 是否存在、是否能解析出 `server_addr`
4. TCP 探测是否成功
5. 可选 HTTP 健康检查是否成功
6. frpc 日志中是否出现常见连接失败或配置错误

### failover

只在离线时动作。

流程：

1. 看节点缓存是否过期，必要时刷新
2. 先对当前节点做一次“固定隧道同步检查”
3. 运行健康检查
4. 如果在线，退出
5. 如果离线，挑出候选节点
6. 逐个尝试切换
7. 每失败一个就短期 ban

如果执行器明确返回“节点级代理冲突/服务端残留冲突”，则不是短期 ban，而是按长期不可用策略冷却该节点。

### fastest

主动选当前最优节点。

流程：

1. 检查冷却时间
2. 看节点缓存是否过期，必要时刷新
3. 测速所有候选节点
4. 取分数最低的节点
5. 如果和当前节点一样，就不动
6. 否则调用执行器切换

---

## 节点为什么会被排除

一个节点不进候选池，通常只会发生在下面这些阶段：

### 阶段 1：根本没出现在全量节点里

看 `chmlfrp-nodes-all.txt`

如果这里没有它，那就是官方 `/node` 当下就没给。

### 阶段 2：被本地筛选条件过滤

看 `chmlfrp-nodes-filtered.txt`

常见原因：

- `FILTER_BUILD_SITE="yes"`，但节点 `web=no`
- `FILTER_TYPE` 不匹配
- `FILTER_CHINA` 不匹配
- `FILTER_NOTES` 没命中

### 阶段 3：nodeinfo 不在线

在测速阶段，如果 `nodeinfo.state != online`，会直接跳过。

### 阶段 4：没有可用的 ping 目标

脚本会按下面的顺序找 IP：

1. `realIp`
2. 本地缓存里的 `节点本地IPv4`
3. `domain_ip`

都没有就跳过。

### 阶段 5：ping 失败或丢包太高

这类节点会被记录到测速日志，但不会进最终候选。

### 阶段 6：被 ban

如果本轮刚切过它并失败了，它会在 `chmlfrp-ban-state.txt` 里短期存在。

如果某节点在 frpc 成功登录后，连续多个 proxy 出现 `router config conflict` / `already exists`，它也会被判定为“节点级不可用”，并进入更长时间的冷却。

---

## 当前最容易踩坑的地方

### 1. `chmlfrp-nodes-filtered.txt` 不是全量节点表

这是当前最常见误解。

如果你拿它去判断“官方到底有哪些节点”，结论经常会错。

正确做法：

- 先看 `chmlfrp-nodes-all.txt`
- 再看 `chmlfrp-nodes-filtered.txt`

### 2. 删除隧道现在不能当成功能前提

脚本里还保留着删除逻辑，是因为历史上确实这么做过。

但现实是：

- V2 删除接口官方说不可用
- V1 删除接口当前实测返回 404

所以现在如果你改“全量同步”逻辑，一定要先考虑：

> 没法稳定删旧隧道时，新隧道创建应该怎么处理？

### 3. 旧缓存会误导排障

以前只有一个筛选后节点文件，容易出现这种情况：

- 官方节点已经变了
- 本地缓存没刷新
- 结果误以为“脚本筛掉了某节点”

现在已经拆成全量和筛选两层，就是为了解决这个问题。

---

## 维护时先看哪几个文件

如果你是人，或者是下一位 AI，建议按这个顺序看：

1. `README.md`
2. `settings.env`
3. `fixed_tunnels.txt`
4. `userdata.txt`
5. `chmlfrp.sh`
6. `new_fix_flow.sh`

排障时优先看：

1. `chmlfrp-health-status.txt`
2. `chmlfrp-node-refresh-state.txt`
3. `chmlfrp-nodes-all.txt`
4. `chmlfrp-nodes-filtered.txt`
5. `chmlfrp-node-metrics.txt`
6. `chmlfrp-tunnels-raw.txt`
7. `chmlfrp-tunnels-normalized.txt`
8. `cloudflare-dns-normalized.txt`
9. `日志-新修复流程.log`

---

## 推荐命令

### 刷新节点缓存

```bash
bash chmlfrp.sh nodes
```

### 只做健康检查

```bash
bash chmlfrp.sh health
```

### 只做当前节点同步修复

```bash
bash chmlfrp.sh reconcile
```

`reconcile` 会在当前节点代理层异常时强制重建固定隧道；如果只是普通的固定隧道同步检查且配置未变化，则仍可能快速退出。

### 离线时自动切换

```bash
bash chmlfrp.sh failover
```

### 主动切到最快节点

```bash
bash chmlfrp.sh fastest
```

### 手动指定节点

```bash
bash chmlfrp.sh manual "湖北襄阳电信"
```

### 刷新 OAuth2 token

```bash
bash chmlfrp.sh oauth_refresh
```

### 重新授权

```bash
bash chmlfrp.sh oauth_reauth
```

---

## 日志风格说明

这次整理后，日志尽量统一成下面这几类：

### 1. 普通流程日志

格式：

```text
[时间][INFO] 文本
```

用于描述主流程推进，比如：

- 读取配置
- 进入哪个步骤
- 当前节点是谁
- 当前模式是什么

### 2. API 请求日志

格式：

```text
[API][标签][REQUEST] method=... url=... auth=...
```

例如：

```text
[API][tunnel_list][REQUEST] method=GET url=https://cf-v2.uapis.cn/tunnel auth=Bearer eyJr***
```

规则：

- token 不打印明文，只打印前 4 位
- URL 尽量打印真实调用地址
- 认证方式明确写出来

### 3. API 响应日志

格式：

```text
[API][标签][RESPONSE] code=... state=... msg=...
```

如果返回不是 JSON，会写成：

```text
[API][标签][RESPONSE] non_json preview=...
```

这样排障时不用再肉眼猜“到底是接口错了，还是脚本没解析对”。

### 4. 节点筛选日志

测速阶段统一写成两类：

#### 被跳过

```text
[NODE][SKIP] name=中国广州 reason=ping_failed ping_target=1.14.160.210
```

#### 入选候选

```text
[NODE][CANDIDATE] name=中国北京-2 score=11 avg_ms=11 loss_pct=0 ping_target=39.96.176.191 ip_for_dns=39.96.176.191
```

这样你一眼就能看出来：

- 某个节点为什么没入选
- 入选节点的评分依据是什么

### 5. 读日志时的顺序

如果一次切换失败，建议按这个顺序看：

1. 先看是否拿到了 `access_token`
2. 再看 `[API][tunnel_list][REQUEST/RESPONSE]`
3. 再看 `[NODE][SKIP]` / `[NODE][CANDIDATE]`
4. 再看 `[API][delete_tunnel][REQUEST/RESPONSE]`
5. 最后看 frpc 配置写入和容器重启日志

这个顺序基本能覆盖 90% 的问题。

---

## 如果你准备继续改代码

下面这些方向是安全的：

### 可以直接改

- README
- 缓存文件字段补充
- 节点筛选条件说明
- 健康检查诊断信息
- 日志文案
- `fastest` / `failover` 的可观测性

### 改之前先确认

- 删除隧道逻辑
- 全量同步模式
- 隧道数量上限处理
- OAuth2 token 刷新策略
- `new_fix_flow.sh` 中的创建/清理顺序

### 最不应该再假设的事

- “V1 删除隧道还能继续用”
- “V2 删除隧道很快就能用”
- “筛选后节点文件等于官方全量节点”

---

## 这次重构后，和之前相比有什么变化

### 已经完成的变化

1. 节点缓存拆成了两层
   - 全量节点
   - 筛选后节点

2. 节点刷新状态单独落盘

3. 测速结果改成 JSON

4. 隧道 / DNS 快照统一成 `.txt + JSON`

5. 固定隧道标准化结果单独保留，不再只当临时文件

6. 执行器不再清理这些快照，方便排障和接手

7. 指定节点与当前节点反查，优先查全量节点缓存，减少被过滤条件误伤

### 仍然存在的现实限制

1. 删除隧道接口不稳定
2. 创建隧道仍受平台 16 条上限约束
3. 某些节点虽然在线，但 `nodeinfo` 或 ping 仍可能超时

---

## 最后一句

如果后面还要继续改，我建议始终遵守一个原则：

> 任何一个运行态文件，都要让人一眼看出它到底是“输入”“缓存”“状态”还是“诊断”。

只要这一点守住，后面不管换人还是换 AI，接手成本都会低很多。
