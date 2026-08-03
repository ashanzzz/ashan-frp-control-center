# Ashan FRP Control Center v1.1.0

个人自托管的 ChmlFrp 高可用控制中心。该版本将官方 `frpc` 直接内置到控制中心镜像中，并由应用自己的 Supervisor 管理，不再依赖第二个 frpc Docker 容器、Docker Socket 或 Unraid 容器控制 Mutation。

## 运行结构

```text
Ashan FRP 单容器
├── Node.js 控制中心
│   ├── Web UI
│   ├── REST API / SSE
│   ├── SQLite / Job Runner
│   ├── OAuth / 凭据保险库
│   └── FRPC Supervisor
└── /usr/local/bin/frpc
    ├── 配置：/data/frpc/conf/frpc.toml
    ├── 日志：/data/frpc/logs/frpc.log
    └── 备份：/data/backups/frpc
```

控制中心负责启动、停止、重启、异常拉起和优雅关闭 `frpc`。节点切换时，系统会先使用 `frpc verify -c` 验证新配置，再原子替换配置并重启内置进程。官方 frp 文档支持使用 `frpc verify -c <config>` 校验配置，并使用 `frpc -c <config>` 启动客户端。

## 与 v1.0.0 的变化

- 删除“通过 Unraid GraphQL 控制独立 frpc 容器”的核心依赖。
- Docker 镜像内置官方 frpc v0.70.1。
- 新增本地进程 Supervisor：自动启动、自动重启、优雅停止、崩溃退避重启。
- FRPC 配置、日志和备份统一持久化在 `/data`。
- 运行时页面显示 PID、版本、运行时长、最后退出状态和日志。
- Unraid API 保留为可选诊断集成，不参与 FRPC 生命周期。
- 默认使用 Host 网络，便于 `frpc` 访问 Unraid 本机及局域网服务。

## Docker Compose 部署

```bash
unzip ashan-frp-control-center-v1.1.0.zip
cd ashan-frp-control-center-v1.1.0
docker compose up -d --build
docker compose logs -f ashan-frp-control
```

访问：

```text
http://UNRAID-IP:8080
```

默认使用：

```yaml
network_mode: host
volumes:
  - ./data:/data
```

不再需要：

```text
/var/run/docker.sock
/mnt/user/appdata/frpc:/host/frpc
独立 frpc 容器
```

## 为什么推荐 Host 网络

内置 `frpc` 与控制中心处于同一个容器网络命名空间。若隧道配置中的 `localIP` 使用 `127.0.0.1`，Host 网络可让它访问 Unraid 主机上的本地服务。使用 Bridge 网络时，应把 `localIP` 改成实际的宿主机或目标容器地址。

## 数据目录

```text
/data/state.db                  SQLite
/data/master.key               凭据加密主密钥
/data/frpc/conf/frpc.toml       当前 FRPC 配置
/data/frpc/logs/frpc.log        FRPC 与 Supervisor 日志
/data/backups/frpc/             历史配置备份
```

只需持久化 `/data`。

## 内置 FRPC 生命周期

### 自动启动

当以下条件同时满足时，控制中心启动后自动拉起 `frpc`：

- `runtime.autostart = true`
- 配置文件存在
- 基础结构校验通过
- `frpc verify -c` 通过

首次安装尚未获得 ChmlFrp 配置时，控制中心保持运行，等待第一次节点切换或配置安装。

### 异常重启

当 `frpc` 非人工停止而异常退出，且 `runtime.auto_restart = true` 时，Supervisor 使用指数退避重新启动，最大等待 60 秒。

### 配置切换

```text
下载目标节点配置
→ 写入临时文件
→ frpc verify
→ 备份旧配置
→ 原子替换
→ 重启内置 frpc
→ 多层健康检查
→ 失败则恢复配置、隧道和 DNS
```

## 直接运行

需要 Node.js 22.5+ 和本机可执行的 `frpc`：

```bash
FRPC_BINARY_PATH=/usr/local/bin/frpc ./scripts/run-direct.sh
```

可覆盖：

```text
DATA_DIR
HTTP_PORT
FRPC_BINARY_PATH
FRPC_CONFIG_PATH
FRPC_BACKUP_DIR
FRPC_LOG_PATH
```

## 前端页面

- 总览
- 连接中心
- 认证中心
- 节点与切换
- 隧道对账
- DNS 对账
- 内置 FRPC 运行时
- 自动化策略
- API 诊断
- 任务中心
- 缓存与快照
- 审计日志

## Unraid API

Unraid GraphQL 现在是可选项，可用于查看系统和容器信息、执行诊断查询。FRPC 启停不再要求 Unraid API Key，也不调用任何 Docker Mutation。

## 构建校验

```bash
npm run verify
```

验证包括前端 TypeScript 编译，以及完整集成测试：ChmlFrp、Cloudflare、节点切换、配置验证、内置 frpc 进程启动、健康检查和失败回滚。
