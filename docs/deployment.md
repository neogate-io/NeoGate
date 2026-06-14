# Deployment Guide

NeoGate can start as a simple single-node deployment and later move to a clustered deployment when traffic requires it.

## Standalone Docker Compose

Standalone deployment is the fastest path for evaluation and small production environments:

```bash
docker compose up -d --build
```

It starts:

- Frontend Nginx
- Backend API and worker
- PostgreSQL

## Source Deployment

Build and run the backend:

```bash
cd backend
cargo build --release
BIND_ADDR=127.0.0.1:8080 ./target/release/neogate
```

Build the frontend:

```bash
cd frontend
pnpm install
pnpm build
```

Serve `frontend/dist` with Nginx and proxy API traffic to the backend.

## Clustered Deployment

Clustered deployment is intended for multi-replica environments:

```bash
cp deploy/env/cluster.env.example .env.cluster
docker compose --env-file .env.cluster -f docker-compose.cluster.yml up -d --build
```

Prepare external PostgreSQL, Redis, shared secrets, and a stable public base URL before going live.

