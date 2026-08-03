# ─────────────────────────────────────────────────────────────
# Stage 1: Build Vue 3 Frontend
# ─────────────────────────────────────────────────────────────
FROM node:22-bookworm-slim AS frontend-builder

WORKDIR /build
COPY frontend/package*.json ./
RUN npm ci --no-audit --prefer-offline

COPY frontend/ ./
RUN npm run build
# Output: /build/dist/ (goes to public/ in runtime image)

# ─────────────────────────────────────────────────────────────
# Stage 2: Runtime Image
# ─────────────────────────────────────────────────────────────
FROM node:22-bookworm-slim

ARG TARGETARCH
ARG FRP_VERSION=0.70.1
ARG FRP_SHA256_AMD64=333da23d1b9009d7c01638e9ba38cf4600f7d37d393f854e96ee1396adefa9a6
ARG FRP_SHA256_ARM64=3990f396a9a490ee7f0e5f355287750ed41520064ed999eab443b5e9a78d773d
ARG BUILD_DATE
ARG VCS_REF

# OCI labels
LABEL org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.revision="${VCS_REF}" \
      org.opencontainers.image.title="Ashan FRP Control Center" \
      org.opencontainers.image.description="Personal self-hosted ChmlFrp high-availability control plane" \
      org.opencontainers.image.source="https://github.com/ashanzzz/ashan-frp-control-center"

# Install system dependencies + frpc binary
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tini \
    && arch="${TARGETARCH:-$(dpkg --print-architecture)}" \
    && case "$arch" in \
         amd64) frp_arch="amd64"; frp_sha="$FRP_SHA256_AMD64" ;; \
         arm64) frp_arch="arm64"; frp_sha="$FRP_SHA256_ARM64" ;; \
         *) echo "Unsupported architecture: $arch" >&2; exit 1 ;; \
       esac \
    && curl -fsSL -o /tmp/frp.tar.gz \
         "https://github.com/fatedier/frp/releases/download/v${FRP_VERSION}/frp_${FRP_VERSION}_linux_${frp_arch}.tar.gz" \
    && echo "${frp_sha}  /tmp/frp.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/frp.tar.gz -C /tmp \
    && install -m 0755 "/tmp/frp_${FRP_VERSION}_linux_${frp_arch}/frpc" /usr/local/bin/frpc \
    && frpc --version \
    && rm -rf /tmp/frp* /var/lib/apt/lists/*

WORKDIR /app

# Install server dependencies (only devDependencies=false for production)
COPY package.json package-lock.json ./
RUN npm ci --only=production --no-audit --prefer-offline 2>/dev/null || npm ci --no-audit --prefer-offline

# Copy server source
COPY src/server ./src/server

# Copy built frontend from stage 1
COPY --from=frontend-builder /build/dist ./public

# Pre-create data directories
RUN mkdir -p /data/frpc/conf /data/frpc/logs /data/backups/frpc

# Environment defaults
ENV HTTP_HOST=0.0.0.0 \
    HTTP_PORT=8080 \
    DATA_DIR=/data \
    PUBLIC_DIR=/app/public \
    FRPC_BINARY_PATH=/usr/local/bin/frpc \
    FRPC_CONFIG_PATH=/data/frpc/conf/frpc.toml \
    FRPC_BACKUP_DIR=/data/backups/frpc \
    FRPC_LOG_PATH=/data/frpc/logs/frpc.log \
    NODE_NO_WARNINGS=1

EXPOSE 8080
VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:${HTTP_PORT}/api/v1/auth/status >/dev/null || exit 1

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["node", "--experimental-strip-types", "src/server/index.ts"]
