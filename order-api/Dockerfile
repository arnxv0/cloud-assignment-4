FROM rust:1.90-slim AS builder

WORKDIR /app

RUN apt-get update && \
    apt-get install -y pkg-config && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./

RUN mkdir -p src && \
    echo 'fn main() { println!("dummy"); }' > src/main.rs && \
    cargo build --release && \
    rm -rf src

COPY src ./src
COPY migrations ./migrations

RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN useradd -m -s /bin/sh app

USER app
WORKDIR /app

COPY --from=builder --chown=app:app /app/target/release/order-api ./order-api

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=5s --start-period=60s --retries=5 \
    CMD curl -f http://localhost:8080/health || exit 1

CMD ["./order-api"]
