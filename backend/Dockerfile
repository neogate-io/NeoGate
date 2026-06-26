ARG DOCKER_HUB_PREFIX=
ARG RUST_IMAGE=${DOCKER_HUB_PREFIX}library/rust:1.94-bookworm
ARG DEBIAN_IMAGE=${DOCKER_HUB_PREFIX}library/debian:bookworm-slim

FROM ${RUST_IMAGE} AS builder

ARG CARGO_REGISTRY_MIRROR=
ARG CARGO_BUILD_JOBS=auto

WORKDIR /app/neogate

# 只在设置了国内镜像时才替换 Cargo 源，否则走官方 crates.io
RUN if [ -n "${CARGO_REGISTRY_MIRROR}" ]; then \
      mkdir -p /usr/local/cargo \
      && printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "%s"\n' "${CARGO_REGISTRY_MIRROR}" > /usr/local/cargo/config.toml; \
    fi

RUN printf '%s\n' \
      '#!/bin/sh' \
      'set -eu' \
      'if [ "${CARGO_BUILD_JOBS}" = "auto" ]; then' \
      '  mem_kb="$(awk '\''/MemTotal/ {print $2}'\'' /proc/meminfo)"' \
      '  cpu_count="$(nproc)"' \
      '  if [ "${mem_kb}" -lt 3145728 ]; then' \
      '    jobs=1' \
      '  elif [ "${mem_kb}" -lt 6291456 ]; then' \
      '    jobs=2' \
      '  else' \
      '    mem_jobs="$((mem_kb / 3145728))"' \
      '    if [ "${mem_jobs}" -lt "${cpu_count}" ]; then jobs="${mem_jobs}"; else jobs="${cpu_count}"; fi' \
      '  fi' \
      'else' \
      '  jobs="${CARGO_BUILD_JOBS}"' \
      'fi' \
      'echo "Using cargo build jobs: ${jobs}"' \
      'exec cargo build --release -j "${jobs}" "$@"' \
      > /usr/local/bin/cargo-build-release \
    && chmod +x /usr/local/bin/cargo-build-release

COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml ./backend/Cargo.toml
COPY scheduler/Cargo.toml ./scheduler/Cargo.toml

# 先用最小占位源码预编译依赖，避免每次业务代码改动都从头编译整个依赖图。
RUN mkdir -p backend/src scheduler/src \
    && printf 'fn main() {}\n' > backend/src/main.rs \
    && printf '\n' > backend/src/lib.rs \
    && printf 'fn main() {}\n' > scheduler/src/main.rs

RUN --mount=type=cache,sharing=locked,target=/usr/local/cargo/registry \
    --mount=type=cache,sharing=locked,target=/usr/local/cargo/git \
    --mount=type=cache,sharing=locked,target=/app/neogate/target \
    cargo-build-release -p neogate -p neogate-scheduler \
    && rm -f target/release/neogate target/release/neogate.d \
      target/release/neogate-scheduler target/release/neogate-scheduler.d \
    && rm -rf backend/src scheduler/src

COPY backend/migrations ./backend/migrations
COPY backend/src ./backend/src
COPY backend/templates ./backend/templates
COPY scheduler/src ./scheduler/src
COPY deploy/systemd/start-neogate.sh ./deploy/systemd/start-neogate.sh

RUN --mount=type=cache,sharing=locked,target=/usr/local/cargo/registry \
    --mount=type=cache,sharing=locked,target=/usr/local/cargo/git \
    --mount=type=cache,sharing=locked,target=/app/neogate/target \
    cargo-build-release -p neogate -p neogate-scheduler \
    && mkdir -p /app/neogate/bin \
    && cp target/release/neogate target/release/neogate-scheduler /app/neogate/bin/

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
COPY --from=builder /app/neogate/bin/neogate /usr/local/bin/neogate
COPY --from=builder /app/neogate/bin/neogate-scheduler /usr/local/bin/neogate-scheduler
COPY --from=builder /app/neogate/deploy/systemd/start-neogate.sh /usr/local/bin/start-neogate
RUN chmod +x /usr/local/bin/start-neogate

VOLUME ["/app-config"]
USER neogate
EXPOSE 8080

CMD ["neogate"]
