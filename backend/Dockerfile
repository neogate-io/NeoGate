ARG DOCKER_HUB_PREFIX=docker.m.daocloud.io/
ARG RUST_IMAGE=${DOCKER_HUB_PREFIX}library/rust:1.94-bookworm
ARG DEBIAN_IMAGE=${DOCKER_HUB_PREFIX}library/debian:bookworm-slim

FROM ${RUST_IMAGE} AS builder

ARG CARGO_REGISTRY_MIRROR=sparse+https://mirrors.ustc.edu.cn/crates.io-index/
ARG CARGO_HTTP_TIMEOUT=600
ARG CARGO_HTTP_LOW_SPEED_LIMIT=1
ARG CARGO_NET_RETRY=10

WORKDIR /app/backend

RUN mkdir -p /usr/local/cargo \
    && printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "%s"\n\n[net]\nretry = %s\n\n[http]\ntimeout = %s\nlow-speed-limit = %s\nmultiplexing = false\n' "${CARGO_REGISTRY_MIRROR}" "${CARGO_NET_RETRY}" "${CARGO_HTTP_TIMEOUT}" "${CARGO_HTTP_LOW_SPEED_LIMIT}" > /usr/local/cargo/config.toml

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/migrations ./migrations
COPY backend/src ./src
COPY backend/templates ./templates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release

FROM ${DEBIAN_IMAGE} AS runtime

ARG APT_MIRROR=http://mirrors.tuna.tsinghua.edu.cn/debian

RUN sed -i "s|http://deb.debian.org/debian|${APT_MIRROR}|g; s|http://deb.debian.org/debian-security|${APT_MIRROR}-security|g" /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
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
