# AITokenPool — multi-stage release image (v0.6.6)
#
# Build: docker build -t aitokenpool:0.6.6 .
# Run:   docker run -p 8080:8080 -v "$PWD/atp-data:/data" \
#          -e ATP_MASTER_KEY="$(openssl rand -hex 32)" \
#          aitokenpool:0.6.6
#
# Notes:
#   - unified data dir (rant 2026-08-19T20:53:23): ATP_DATA_DIR=/data holds
#     config.toml + aitokenpool.db + logs/ — mount ONE volume.
#   - first start copies /config/config.example.toml → /data/config.toml
#     if missing (main.rs ensure_config).
#   - master_key comes from env ATP_MASTER_KEY (32-byte hex), crypto reads env first.

# ---------- builder ----------
FROM rust:1.86-slim AS builder
WORKDIR /build
# native-tls (openssl-sys) 需要系统 OpenSSL 头文件
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
# 依赖层缓存：源码变更不重编依赖
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY ui ./ui
# main.rs include_str!("../config/config.example.toml") 需要编译期存在该文件（a39e809 内嵌默认配置）
COPY config ./config
RUN cargo build --release --locked

# ---------- runtime ----------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home --shell /usr/sbin/nologin app

WORKDIR /
COPY --from=builder /build/target/release/aitokenpool /usr/local/bin/aitokenpool
COPY ui /ui
# 内置示例配置：首次启动时由 main.rs 复制到 /data/config.toml（若不存在）
COPY config/config.example.toml /config/config.example.toml

RUN mkdir -p /data /config \
    && chown -R 10001:10001 /data

USER 10001
ENV ATP_DATA_DIR=/data
EXPOSE 8080
VOLUME ["/data"]
CMD ["aitokenpool", "--data-dir", "/data"]
