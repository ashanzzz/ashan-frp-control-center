# syntax=docker/dockerfile:1
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget tzdata \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY .release/ashan-frp-server /app/ashan-frp-server
COPY .release/web/ /app/web/

RUN mkdir -p /data/frpc /data/logs \
    && chmod +x /app/ashan-frp-server

ENV HTTP_ADDR=0.0.0.0:8080 \
    DATA_DIR=/data \
    DATABASE_URL=sqlite:///data/control-center.db \
    WEB_DIR=/app/web \
    FRPC_BINARY=/data/frpc/frpc \
    FRPC_CONFIG=/data/frpc/frpc.ini

EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD wget -qO- http://127.0.0.1:8080/api/v1/health >/dev/null 2>&1 || exit 1

ENTRYPOINT ["/app/ashan-frp-server"]
