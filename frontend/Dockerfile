ARG DOCKER_HUB_PREFIX=docker.m.daocloud.io/
ARG NODE_IMAGE=${DOCKER_HUB_PREFIX}library/node:20-alpine
ARG NGINX_IMAGE=${DOCKER_HUB_PREFIX}library/nginx:1.27-alpine

FROM ${NODE_IMAGE} AS builder

WORKDIR /app/frontend

ENV COREPACK_NPM_REGISTRY=https://registry.npmmirror.com
RUN corepack enable

COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile --registry https://registry.npmmirror.com

COPY frontend ./
RUN pnpm build

FROM ${NGINX_IMAGE} AS runtime

COPY deploy/nginx/compose.conf.template /etc/nginx/templates/default.conf.template
COPY --from=builder /app/frontend/dist /usr/share/nginx/html

ENV API_UPSTREAM=http://backend:8080

EXPOSE 80
