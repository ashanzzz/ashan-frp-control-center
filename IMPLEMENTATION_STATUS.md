# 实现状态 — v1.1.0

## 已完成

- 前后端一体化单容器部署。
- 官方 frpc v0.70.1 内置到镜像。
- Node Supervisor 管理内置 frpc：启动、停止、重启、异常重启和优雅退出。
- 使用 `frpc verify -c` 进行真实配置校验。
- 配置临时写入、备份、原子替换和回滚。
- FRPC PID、版本、运行时长、日志和退出状态展示。
- ChmlFrp OAuth、节点、隧道同步与增量对账。
- Cloudflare DNS 增量对账和原生记录保护。
- 节点测试、评分、Ban、手动及自动故障切换。
- 持久任务、SSE、审计、缓存和凭据加密。
- 完整集成测试使用模拟 frpc 进程验证切换与回滚。

## 不再需要

- 独立 frpc Docker 容器。
- Docker Socket。
- 挂载其他 frpc Appdata。
- Unraid Docker 启停 Mutation。

## 实机上线仍需确认

- ChmlFrp 返回的实际 `frpc.toml` 与内置 frpc v0.70.1 兼容。
- Host 网络下所有隧道的 `localIP/localPort` 可达。
- Cloudflare Zone 和 Token 权限正确。
- 自动切换阈值符合实际网络质量。
