# syntax=docker/dockerfile:1
FROM rust:1.94-bookworm AS frontend
RUN cargo install dioxus-cli --version '^0.7' --locked
WORKDIR /src
COPY . .
RUN rustup target add wasm32-unknown-unknown
RUN dx build --release --platform web
RUN mkdir -p /out/web && cp -a dist/public/. /out/web/

FROM rust:1.94-bookworm AS backend
WORKDIR /src
COPY . .
RUN cargo build --release -p ashan-frp-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget tzdata && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=backend /src/target/release/ashan-frp-server /app/ashan-frp-server
COPY --from=frontend /out/web /app/web
RUN mkdir -p /data/frpc /data/logs
ENV HTTP_ADDR=0.0.0.0:8080 \
    DATA_DIR=/data \
    DATABASE_URL=sqlite:///data/control-center.db \
    WEB_DIR=/app/web \
    FRPC_BINARY=/data/frpc/frpc \
    FRPC_CONFIG=/data/frpc/frpc.ini
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 CMD wget -qO- http://127.0.0.1:8080/api/v1/health >/dev/null 2>&1 || exit 1
ENTRYPOINT ["/app/ashan-frp-server"]
