# Build stage
FROM rust:1.94-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p telegram_bot

# Runtime stage
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/telegram_bot /usr/local/bin/telegram_bot
WORKDIR /app
# The vault (secrets + sqlite storage) is mounted at runtime; see docker-compose.yml.
ENV YCB_VAULT_DIR=/app/config/vault
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD curl -fsS http://127.0.0.1:3000/health || exit 1
CMD ["telegram_bot"]
