FROM rust:1.85-bookworm AS builder

WORKDIR /app/backend

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/migrations ./migrations
COPY backend/src ./src

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --create-home --home-dir /app moligate

WORKDIR /app
COPY --from=builder /app/backend/target/release/moligate /usr/local/bin/moligate

USER moligate
EXPOSE 8080

CMD ["moligate"]
