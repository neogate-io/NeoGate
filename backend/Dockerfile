FROM rust:1.94-bookworm AS builder

WORKDIR /app/backend

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/migrations ./migrations
COPY backend/src ./src
COPY backend/templates ./templates

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --create-home --home-dir /app neogate \
    && mkdir -p /app-config \
    && chown neogate:neogate /app-config

WORKDIR /app
COPY --from=builder /app/backend/target/release/neogate /usr/local/bin/neogate

VOLUME ["/app-config"]
USER neogate
EXPOSE 8080

CMD ["neogate"]
