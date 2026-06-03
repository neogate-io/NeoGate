# NeoGate

[中文文档](README.zh-CN.md)

NeoGate is a minimal Rust/Axum LLM API gateway.

It provides the backend foundation for:

- users and downstream `user_key` authentication
- upstream `channel` management
- per-channel upstream `channel_key` pools
- OpenAI-compatible and Anthropic-compatible relay routes
- usage recording and basic upstream key cooldown

This repository intentionally starts with backend-only infrastructure. There is no frontend, payment system, or complex user console in this first framework.

## Database Model

The PostgreSQL schema is intentionally small:

- `"user"`: gateway users
- `user_key`: downstream API keys used by clients calling NeoGate
- `wallet`: credit balances for users and user keys
- `channel`: upstream provider service groups, with priority, weight, and key selection mode
- `channel_endpoint`: protocol-specific upstream endpoints inside a channel, with protocol, Base URL, model allowlist, and health state
- `channel_key`: upstream API keys inside each channel
- `provider`: upstream provider catalog entries, including default OpenAI-compatible and Anthropic-compatible endpoint settings
- `usage`: relay usage records
- `billing`: durable billing outbox
- `credit_allocation` and `credit_ledger`: wallet allocation state and immutable credit ledger entries

`"user"` is quoted in SQL because `user` is a PostgreSQL keyword.

## Quick Start

Create a database:

```bash
createdb neogate
```

Run the backend:

```bash
cp .env.example .env
# edit .env, especially DATABASE_URL and admin secrets
cd backend
cargo run
```

Optional SMTP delivery for public API key claims is configured in `.env`:

```dotenv
MAIL_SMTP_HOST=smtp.example.com
MAIL_SMTP_PORT=587
MAIL_SMTP_USERNAME=apikey
MAIL_SMTP_PASSWORD=secret
MAIL_FROM_EMAIL=noreply@example.com
MAIL_FROM_NAME=
MAIL_SUBJECT_PREFIX=
```

`MAIL_SMTP_TLS` defaults to `true`. `MAIL_FROM_NAME` and `MAIL_SUBJECT_PREFIX` are optional; when left empty, the backend uses `NeoGate` for all public API key emails. `.env` is gitignored and should hold the real SMTP password.

## Frontend Install Script

The admin frontend uses same-origin `/api` requests. In local development, Vite only proxies `/api` to the Rust backend. The install script configures Codex/Claude to call the Rust backend directly instead of relaying `/v1` or `/anthropic` through Vite.

For production, use Vite's standard env files to set the public backend origin. On the deployment host, copy the template to `.env.production.local`:

```bash
cd frontend
cp .env.example .env.production.local
```

```dotenv
VITE_NEOGATE_BACKEND_ORIGIN=https://api.example.com
```

The generated install script uses `https://api.example.com/v1` for Codex/OpenAI and `https://api.example.com/anthropic` for Claude/Anthropic. You can also pass `VITE_NEOGATE_BACKEND_ORIGIN=https://api.example.com` when building or running Vite.

## Deployment Notes

NeoGate has two runtime modes:

- `RUNTIME_MODE=standalone`: no Redis dependency. Hot credit, cache invalidation, and relay caches are process-local, so run a single backend instance.
- `RUNTIME_MODE=distributed`: multiple stateless backend replicas share Redis for hot credit and cache invalidation. Set `REDIS_URL` to a writable Redis endpoint. This supports a single Redis primary or a Sentinel-managed primary exposed through your infrastructure; Redis Cluster hash-slot sharding is intentionally not supported.

`PROCESS_ROLE` controls which work a process performs:

- `PROCESS_ROLE=all`: serve HTTP and run background billing/recovery workers. This is the default and keeps single-node deployments simple.
- `PROCESS_ROLE=api`: serve HTTP, write billing outbox rows, and flush this process's lightweight usage queue, but do not scan durable billing backlog or run allocation recovery.
- `PROCESS_ROLE=worker`: run background billing/recovery workers without opening the HTTP listener.

Admin login tokens and public API key draft tokens are signed with `ADMIN_TOKEN_SECRET`; they are not stored in process memory. Upstream channel keys are encrypted at rest with `UPSTREAM_SECRET_KEY` and decrypted through a small in-memory cache on the relay path.

For production, set `APP_ENV=production`. Production mode rejects the example admin password/signing secret and upstream encryption secret, and requires both secrets to be at least 32 characters.

Configuration is grouped in `.env.example` as:

- `App`: environment, bind address, and CORS
- `Database`: local PostgreSQL connection and Docker Compose database settings
- `Admin`: admin login and signing secret
- `Relay`: upstream protocol defaults and upstream key encryption secret
- `Mail`: SMTP delivery for public API key claims
- `Advanced optional tuning`: pool size, request timeout, key cooldown, relay caches, and background usage recording

Common production settings:

```dotenv
APP_ENV=production
RUNTIME_MODE=standalone
PROCESS_ROLE=all
CORS_ALLOWED_ORIGINS=https://admin.example.com,https://console.example.com
DB_POOL_MAX_CONNECTIONS=10
RELAY_BODY_LIMIT_BYTES=16777216
HTTP_POOL_MAX_IDLE_PER_HOST=100
HTTP_POOL_IDLE_TIMEOUT_SECONDS=90
USER_AUTH_CACHE_TTL_SECONDS=60
ROUTING_CACHE_TTL_SECONDS=30
PRICE_CACHE_TTL_SECONDS=300
USAGE_FLUSH_INTERVAL_MS=1000
USAGE_QUEUE_SIZE=8192
BILLING_OUTBOX_MAX_PENDING=10000
BILLING_OUTBOX_MAX_AGE_SECONDS=300
CREDIT_ALLOCATION_RECOVERY_AFTER_SECONDS=900
CREDIT_ALLOCATION_RECOVERY_INTERVAL_SECONDS=60
DEFAULT_OUTPUT_TOKENS=2048
```

For multi-replica deployments:

```dotenv
RUNTIME_MODE=distributed
REDIS_URL=redis://redis.example.com:6379/
REDIS_KEY_PREFIX=neogate
```

For simple multi-replica deployments, run multiple `PROCESS_ROLE=api` replicas behind the load balancer and at least one `PROCESS_ROLE=worker` replica for billing backlog processing and allocation recovery.

Health probes:

- `GET /healthz`: process liveness
- `GET /readyz`: readiness, including PostgreSQL connectivity, Redis connectivity when `RUNTIME_MODE=distributed`, billing outbox write health, and billing backlog thresholds

Billing settlements are inserted into the durable `billing` outbox with short retry, then finalized by a background worker in batches. Non-billing relay usage is still queued in memory and flushed to PostgreSQL asynchronously. If a non-billing usage flush fails, the in-process batch is kept for retry; a process crash can lose unflushed non-billing usage records. Stale active credit allocations are periodically recovered after `CREDIT_ALLOCATION_RECOVERY_AFTER_SECONDS`; before recovery, NeoGate removes matching hot-credit entries from the configured runtime store.

Container startup:

```bash
cp .env.example .env
# edit .env and replace every change-me value before production use
docker compose up --build
```

Login (`POST /api/login`) returns a generated session token. Admin endpoints accept:

- `Authorization: Bearer <login_session_token>`

Gateway relay endpoints accept either:

- `Authorization: Bearer <user_key>`
- `x-api-key: <user_key>`

## Relay Routes

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/messages`

OpenAI-compatible requests are forwarded with:

```text
Authorization: Bearer <decrypted channel key>
```

Anthropic-compatible requests are forwarded with:

```text
x-api-key: <decrypted channel key>
anthropic-version: <incoming value or ANTHROPIC_VERSION>
```

## Minimal Admin Flow

1. Create a user with `POST /api/admin/users`.
2. Create a downstream key with `POST /api/admin/user-keys`.
3. Create an upstream channel with `POST /api/admin/channels`.
4. Add upstream keys with `POST /api/admin/channels/:id/keys`.
5. Configure input/output prices for the fetched upstream models on the Prices page.
6. Call the relay routes with the generated downstream key.

## Selection Rules

For each relay request, NeoGate:

1. validates the downstream `user_key`
2. filters channel endpoints by provider, protocol, requested model, enabled state, health, cooldown, and available keys on the parent channel
3. chooses the highest-priority channel tier
4. chooses a channel by weight inside that tier
5. chooses a channel key by `polling` or `random`
6. cools down the selected channel key on upstream failure

## Verification

```bash
cd backend
cargo test
cargo check
```
