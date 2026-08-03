# 安全设计 — v1.1.0

- 不挂载 Docker Socket。
- 不需要特权模式。
- FRPC 二进制在镜像构建时从官方 Release 下载并校验 SHA-256。
- 配置文件、日志和备份位于 `/data`，默认以 `0600` 创建关键文件。
- 新配置必须通过 `frpc verify -c` 才能安装。
- 容器使用 `tini` 转发终止信号并回收子进程。
- Token 和 API Key 使用 AES-256-GCM 加密保存在 SQLite。
- 完整凭据查看需要再次验证管理员密码，不写 localStorage。
- Unraid API 已降级为可选诊断集成，FRPC 控制不需要 Unraid 管理权限。
