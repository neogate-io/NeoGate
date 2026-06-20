ARG DOCKER_HUB_PREFIX=
ARG NODE_IMAGE=${DOCKER_HUB_PREFIX}library/node:20-alpine
ARG NGINX_IMAGE=${DOCKER_HUB_PREFIX}library/nginx:1.27-alpine

FROM ${NODE_IMAGE} AS builder

# 默认使用官方 npm registry。国内网络可设置 NPM_REGISTRY=https://registry.npmmirror.com
ARG NPM_REGISTRY=

WORKDIR /app/frontend

RUN corepack enable

COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN if [ -n "${NPM_REGISTRY}" ]; then \
      pnpm install --frozen-lockfile --registry "${NPM_REGISTRY}"; \
    else \
      pnpm install --frozen-lockfile; \
    fi

COPY frontend ./
RUN pnpm build

FROM ${NGINX_IMAGE} AS runtime

COPY frontend/nginx/default.conf.template /etc/nginx/templates/default.conf.template
COPY --from=builder /app/frontend/dist /usr/share/nginx/html

ENV API_UPSTREAM=http://backend:8080

EXPOSE 80
