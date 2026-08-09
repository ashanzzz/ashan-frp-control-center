# ADR-0001: Rust Fullstack with Dioxus + Axum

Status: Accepted

## Decision

使用 Dioxus 0.7 Web/WASM 作为前端，Axum 0.8 作为后端，Tokio 作为异步运行时，SQLx + SQLite 作为持久化层。

## Consequences

- Vue/React/Node 不再是产品运行依赖；
- 前后端共享 Rust DTO；
- 保留稳定 REST API，同时允许 UI 使用 Dioxus Server Functions；
- 生产仍为单容器单端口。
