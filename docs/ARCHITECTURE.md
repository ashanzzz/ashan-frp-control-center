# 架构设计 — v1.1.0

## 1. 单容器边界

```text
Browser
  │ HTTP / SSE
  ▼
Node.js Control Plane
  ├── API / Auth / UI
  ├── SQLite / Jobs / Audit
  ├── ChmlFrp Adapter
  ├── Cloudflare Adapter
  ├── Optional Unraid GraphQL Adapter
  └── FRPC Supervisor
          │ child process
          ▼
    /usr/local/bin/frpc -c /data/frpc/conf/frpc.toml
```

FRPC 是控制中心容器内的子进程，不是另一个 Docker 容器。控制中心不需要 Docker Socket，也不依赖 Unraid Docker Mutation。

## 2. 运行时状态机

```text
stopped → starting → running → stopping → stopped
                     │
                     └─ unexpected exit → crashed → backoff restart
```

持久化设置：

- `runtime.autostart`
- `runtime.auto_restart`
- `runtime.binary_path`
- `runtime.config_path`
- `runtime.log_path`
- `runtime.backup_dir`

## 3. 配置提交

新配置先写入临时文件，执行官方 `frpc verify -c`，通过后备份旧配置并原子重命名。任何验证失败都不会覆盖当前配置。

## 4. 故障切换

```text
生成切换计划
→ 重建目标节点隧道
→ 更新受管 DNS
→ 下载目标 frpc.toml
→ 二进制验证
→ 原子安装
→ 重启内置 frpc
→ 多层健康验证
→ 成功提交或完整回滚
```

## 5. 网络模式

默认 Host 网络，原因是大量本地隧道使用 `127.0.0.1` 或 Unraid 局域网地址。Bridge 模式仍可使用，但每条 `localIP` 必须从容器网络可达。
