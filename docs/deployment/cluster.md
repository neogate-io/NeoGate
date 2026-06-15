# Cluster Deployment

This guide is for NeoGate environments that need multiple replicas, horizontal scaling, or production high availability. For first-time evaluation or small self-hosted use, start with the standalone Docker Compose flow in the main README.

## Deployment Layout

The cluster Compose file is `docker-compose.cluster.yml` and includes these services:

| Service | Description |
| --- | --- |
| `web` | Frontend Nginx service that serves static assets and proxies API traffic. |
| `api` | Backend API process for the admin console and model relay requests. |
| `worker` | Backend worker process for background jobs and usage flushing. |

The cluster Compose file does not include PostgreSQL or Redis. All API and worker nodes must connect to the same external PostgreSQL and Redis instances, and they must share the same secrets.

## Prerequisites

Prepare these before going live:

- External PostgreSQL, preferably a managed database or dedicated instance.
- External Redis for cross-node coordination and shared state.
- A stable public URL, such as `https://neogate.example.com`.
- Long random shared secrets that are identical across every API and worker node.
- A host with Docker and Docker Compose available.

## Configure Environment Variables

Copy the cluster environment example:

```bash
cp deploy/env/cluster.env.example .env.cluster
```

Review and update at least these variables:

| Variable | Description |
| --- | --- |
| `DATABASE_URL` | External PostgreSQL connection URL. |
| `REDIS_URL` | External Redis connection URL. |
| `REDIS_KEY_PREFIX` | Redis key prefix. Keep it unique if multiple environments share one Redis instance. |
| `PUBLIC_BASE_URL` | Public site URL, such as `https://neogate.example.com`. |
| `CORS_ALLOWED_ORIGINS` | Allowed frontend origins, usually the same as `PUBLIC_BASE_URL`. |
| `ADMIN_TOKEN_SECRET` | Admin token signing secret. Use a long random value. |
| `UPSTREAM_SECRET_KEY` | Upstream credential encryption secret. Use a long random value. |
| `WEB_PORT` | Frontend port exposed by Docker Compose. Defaults to `8080`. |

> [!WARNING]
> Do not use the example passwords or `change-me` secrets in production. Back up `ADMIN_TOKEN_SECRET` and `UPSTREAM_SECRET_KEY` after production use; changing them can invalidate existing sessions or make saved credentials undecryptable.

## Start the Cluster

Start with `.env.cluster` and the cluster Compose file:

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml up -d --build
```

Both parameters are required:

- `--env-file .env.cluster` tells Docker Compose to load cluster environment variables.
- `-f docker-compose.cluster.yml` tells Docker Compose to use the cluster Compose file instead of the default standalone `docker-compose.yml`.

## Check Runtime Status

Check service status:

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml ps
```

You should normally see `web`, `api`, and `worker`. The `web` and `api` services include health checks and should become `running` or `healthy`.

View all logs:

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml logs -f
```

View API logs only:

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml logs -f api
```

View worker logs only:

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml logs -f worker
```

Finally, open the URL configured by `PUBLIC_BASE_URL`. If the bootstrap wizard or login page loads, the frontend, backend, and reverse proxy path are usually working.

## Common Operations

Stop the cluster:

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml down
```

Rebuild and start:

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml up -d --build
```

Restart the API service:

```bash
docker compose --env-file .env.cluster -f docker-compose.cluster.yml restart api
```

Scale API or worker replicas with your external load balancer and orchestration layer. If you use Docker Compose replicas directly, make sure port exposure, reverse proxy routing, and shared storage match your production requirements.

## Troubleshooting

If services do not become `running` or `healthy`, check:

- `.env.cluster` exists and includes valid `PUBLIC_BASE_URL`, `DATABASE_URL`, and `REDIS_URL`.
- PostgreSQL and Redis allow connections from the deployment host.
- `ADMIN_TOKEN_SECRET` and `UPSTREAM_SECRET_KEY` are not empty and are not still example values.
- `WEB_PORT` is not already used by another process.
- The `api` logs for migration, database connection, or missing configuration errors.

## Production Recommendations

- Use an HTTPS URL for `PUBLIC_BASE_URL`.
- Enable access control, backups, and monitoring for PostgreSQL and Redis.
- Restrict `.env.cluster` permissions to the deployment user.
- Do not commit the production `.env.cluster` file to Git.
- Back up the database before upgrades and validate migrations in a test environment first.
