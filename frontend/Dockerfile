FROM node:20-alpine AS builder

WORKDIR /app/frontend

RUN corepack enable

COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY frontend ./
RUN pnpm build

FROM nginx:1.27-alpine AS runtime

COPY deploy/nginx/default.conf.template /etc/nginx/templates/default.conf.template
COPY deploy/nginx/render-install.sh /docker-entrypoint.d/20-render-install.sh
COPY frontend/install.template /usr/share/nginx/html/install.template
COPY --from=builder /app/frontend/dist /usr/share/nginx/html
RUN chmod +x /docker-entrypoint.d/20-render-install.sh

ENV API_UPSTREAM=http://backend:8080
ENV NEOGATE_PUBLIC_ORIGIN=http://localhost:8080

EXPOSE 80
