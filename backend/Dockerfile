ARG DOCKER_HUB_PREFIX=
ARG RUST_IMAGE=${DOCKER_HUB_PREFIX}library/rust:1.94-bookworm
ARG DEBIAN_IMAGE=${DOCKER_HUB_PREFIX}library/debian:bookworm-slim

FROM ${RUST_IMAGE} AS builder

ARG CARGO_REGISTRY_MIRROR=
ARG CARGO_BUILD_JOBS=1

WORKDIR /app/neogate

# 只在设置了国内镜像时才替换 Cargo 源，否则走官方 crates.io
RUN if [ -n "${CARGO_REGISTRY_MIRROR}" ]; then \
      mkdir -p /usr/local/cargo \
      && printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "%s"\n' "${CARGO_REGISTRY_MIRROR}" > /usr/local/cargo/config.toml; \
    fi

COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml ./backend/Cargo.toml
COPY scheduler/Cargo.toml ./scheduler/Cargo.toml
COPY backend/migrations ./backend/migrations
COPY backend/src ./backend/src
COPY backend/templates ./backend/templates
COPY scheduler/src ./scheduler/src
COPY deploy/systemd/start-neogate.sh ./deploy/systemd/start-neogate.sh

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release -j ${CARGO_BUILD_JOBS} -p neogate -p neogate-scheduler

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
    && mkdir -p /app/neogate /app-config /var/log/neogate \
    && chown neogate:neogate /app/neogate /app-config /var/log/neogate

WORKDIR /app/neogate
COPY --from=builder /app/neogate/target/release/neogate /usr/local/bin/neogate
COPY --from=builder /app/neogate/target/release/neogate-scheduler /usr/local/bin/neogate-scheduler
COPY --from=builder /app/neogate/deploy/systemd/start-neogate.sh /usr/local/bin/start-neogate
RUN chmod +x /usr/local/bin/start-neogate

VOLUME ["/app-config"]
USER neogate
EXPOSE 8080

CMD ["neogate"]
