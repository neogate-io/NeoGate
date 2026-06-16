ARG DOCKER_HUB_PREFIX=
ARG RUST_IMAGE=${DOCKER_HUB_PREFIX}library/rust:1.94-bookworm
ARG DEBIAN_IMAGE=${DOCKER_HUB_PREFIX}library/debian:bookworm-slim

FROM ${RUST_IMAGE} AS builder

ARG CARGO_REGISTRY_MIRROR=

WORKDIR /app/backend

# 只在设置了国内镜像时才替换 Cargo 源，否则走官方 crates.io
RUN if [ -n "${CARGO_REGISTRY_MIRROR}" ]; then \
      mkdir -p /usr/local/cargo \
      && printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "%s"\n' "${CARGO_REGISTRY_MIRROR}" > /usr/local/cargo/config.toml; \
    fi

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/migrations ./migrations
COPY backend/src ./src
COPY backend/templates ./templates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release

FROM ${DEBIAN_IMAGE} AS runtime

ARG APT_MIRROR=

# 只在设置了国内镜像时才替换 APT 源
RUN if [ -n "${APT_MIRROR}" ]; then \
      sed -i "s|http://deb.debian.org/debian|${APT_MIRROR}|g; s|http://deb.debian.org/debian-security|${APT_MIRROR}-security|g" /etc/apt/sources.list.d/debian.sources; \
    fi \
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
