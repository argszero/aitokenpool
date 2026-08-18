# AITokenPool — multi-stage release image (v0.3.3)
#
# Build: docker build -t aitokenpool:0.3.3 .
# Run:   docker run -p 8080:8080 -v "$PWD/data:/data" \
#          -e ATP_MASTER_KEY="$(openssl rand -hex 32)" \
#          aitokenpool:0.3.3
#
# Notes:
#   - runtime WORKDIR is / so the relative paths in config.example.toml work:
#       db_path = "data/aitokenpool.db"  -> /data/aitokenpool.db (mounted volume)
#       ServeDir "ui"                    -> /ui (copied below)
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
COPY config/config.example.toml /etc/aitokenpool/config.toml

RUN mkdir -p /data /etc/aitokenpool \
    && chown -R 10001:10001 /data /etc/aitokenpool

USER 10001
EXPOSE 8080
VOLUME ["/data"]
CMD ["aitokenpool", "--config", "/etc/aitokenpool/config.toml"]
