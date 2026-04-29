# ─────────────────────────────────────────────
# Stage 1: Build React frontend
# ─────────────────────────────────────────────
FROM node:22-alpine AS frontend
WORKDIR /app
COPY app/package*.json ./
RUN npm ci --prefer-offline --ignore-scripts
COPY app/ ./
RUN npm run build

# ─────────────────────────────────────────────
# Stage 2: Build Rust backend
# ─────────────────────────────────────────────
FROM rust:1.82-slim AS rust-builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache Cargo dependencies separately from source
COPY aetherclaw/Cargo.toml ./Cargo.toml
# Cargo.lock may not exist on first run — let Cargo create it
COPY aetherclaw/Cargo.lock* ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs \
    && cargo build --release 2>/dev/null || true \
    && rm -rf src target/release/aetherclaw target/release/deps/aetherclaw*

# Build for real
COPY aetherclaw/src ./src
RUN cargo build --release

# ─────────────────────────────────────────────
# Stage 3: Minimal runtime image
# ─────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates wget \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 1001 -g root aetherclaw

COPY --from=rust-builder /build/target/release/aetherclaw /usr/local/bin/aetherclaw
COPY --from=frontend    /app/dist                          /app/static

RUN mkdir -p /data /workspace /root/.aetherclaw \
    && chmod +x /usr/local/bin/aetherclaw

VOLUME ["/data", "/workspace", "/root/.aetherclaw"]

ENV RUST_LOG=info
ENV STATIC_DIR=/app/static

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget --spider -q http://localhost:8080/api/health || exit 1

CMD ["aetherclaw"]
