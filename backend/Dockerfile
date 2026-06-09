FROM rust:1.85-bookworm AS builder

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

RUN useradd --system --create-home --home-dir /app neogate

WORKDIR /app
COPY --from=builder /app/backend/target/release/neogate /usr/local/bin/neogate

USER neogate
EXPOSE 8080

CMD ["neogate"]
