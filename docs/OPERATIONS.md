# 运维 — v1.1.0

## 查看状态

```bash
docker compose ps
docker compose logs -f ashan-frp-control
```

网页：`FRPC 运行时` 页面可查看 PID、版本、配置、日志与最后退出信息。

## 持久化备份

备份整个 `/data`：

```text
state.db
master.key
frpc/conf/frpc.toml
frpc/logs/frpc.log
backups/frpc/
```

`master.key` 与 `state.db` 必须成对保存，否则加密凭据无法解密。

## FRPC 无法启动

检查顺序：

1. 运行时页面确认二进制存在及版本。
2. 查看配置校验错误。
3. 查看 `/data/frpc/logs/frpc.log`。
4. 确认 Host 网络和本地目标地址。
5. 手动点击“重启”。

## 迁移旧独立 frpc

停止并禁用旧 frpc 容器，避免同一账户和隧道重复连接。将旧配置作为参考即可；正式配置会在节点切换时从 ChmlFrp 下载并写入 `/data/frpc/conf/frpc.toml`。
