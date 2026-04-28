# Build stage
FROM rust:1.75-alpine AS builder
RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build static binary
ENV RUSTFLAGS="-C target-feature=+crt-static -C strip=symbols"
RUN cargo build --release --target x86_64-unknown-linux-musl

# Runtime stage (scratch for minimal size)
FROM scratch
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/aetherclaw /aetherclaw
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Data volume
VOLUME ["/data"]

ENV PERSISTENCE_PATH=/data/memory.db
ENV RUST_LOG=info

EXPOSE 8080

ENTRYPOINT ["/aetherclaw"]
