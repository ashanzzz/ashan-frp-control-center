# Changelog

## 1.1.0 — Embedded FRPC Runtime

### Added
- 镜像内置官方 frpc v0.70.1，支持 amd64 和 arm64。
- Node FRPC Supervisor：自动启动、手动启停、异常退避重启和优雅关闭。
- 真实 `frpc verify -c` 配置验证。
- FRPC 运行日志 API 和网页查看入口。
- PID、版本、运行时长、期望状态及最后退出状态。

### Changed
- 默认 Host 网络。
- 配置路径改为 `/data/frpc/conf/frpc.toml`。
- FRPC 日志路径改为 `/data/frpc/logs/frpc.log`。
- Unraid API 改为可选诊断，不再控制 FRPC。
- 切换流程重启内置进程，而不是独立 Docker 容器。

### Removed
- Docker Socket 需求。
- 独立 frpc 容器依赖。
- `/host/frpc` 挂载。
- Unraid 容器 Mutation 作为核心运行依赖。

## 1.0.0
- 完成控制中心基础业务、节点、隧道、DNS、任务、认证和故障切换。
