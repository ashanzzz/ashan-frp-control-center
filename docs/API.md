# API — v1.1.0

## 内置 FRPC

### `GET /api/v1/runtime`

返回：

- `mode = embedded`
- `process.state/pid/startedAt/uptimeSeconds`
- `process.desiredState/autoRestart/lastExit`
- `binary.path/version/exists`
- `file.path/hash/exists`
- `validation`
- `log.path`

### `GET /api/v1/runtime/config`

返回当前 `frpc.toml` 及基础结构校验。

### `GET /api/v1/runtime/logs?lines=500`

返回 Supervisor 与 frpc 合并日志尾部，最多 2000 行。

### `POST /api/v1/runtime/action`

```json
{"action":"start"}
```

允许：`start`、`stop`、`restart`。请求进入持久任务队列。

## Unraid API

`/api/v1/providers/unraid/*` 保留用于可选诊断和自定义 GraphQL 查询，不再参与 FRPC 生命周期。
