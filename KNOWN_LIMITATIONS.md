# 已知限制 — v1.1.0

1. Docker 镜像当前构建支持 `linux/amd64` 和 `linux/arm64`。
2. 默认采用 Host 网络；Bridge 网络需要为每条隧道使用可从容器访问的目标地址，不能默认依赖 `127.0.0.1`。
3. 控制中心与 frpc 在同一容器生命周期内。控制中心容器停止时，frpc 同时停止，这是设计行为。
4. ChmlFrp 用户名、密码和邮箱验证码的私有登录端点尚无可信规范；当前保留 Refresh Token、Device Code、人工 Token 和验证码 Webhook路径。
5. 未使用用户的真实 ChmlFrp、Cloudflare 和 Unraid 凭据进行生产写操作测试。
6. 当前环境未提供 Docker daemon，因此未在本会话内执行真实 `docker build`；Dockerfile 的下载地址和 SHA-256 采用官方 frp v0.70.1 Release 数据。
