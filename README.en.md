# NeoGate

[中文文档](README.md)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)

NeoGate is a lightweight LLM API gateway built with Rust. It is designed for high-performance request forwarding while staying simple to deploy and use, helping teams bring multiple model providers behind one unified entry point for access keys, routing, and usage records.

Repository: [neogate-io/NeoGate](https://github.com/neogate-io/NeoGate)

## 1. What You Can Do

- Issue independent API keys to teams, customers, or internal apps without exposing upstream provider keys.
- Manage OpenAI, Anthropic, and other upstream model services from one admin console.
- Route requests by model, priority, weight, allowlist, and key selection policy.
- Expose OpenAI-compatible and Anthropic-compatible endpoints so existing clients can connect with minimal changes.
- Track usage by user and API key for troubleshooting, cost analysis, and future billing.
- Cool down failing upstream keys and continue routing through available keys.
- Let users claim API keys by email for a lightweight self-service model access flow.
- Use the Chinese and English admin UI for self-hosted community deployments and future enterprise controls.

## 2. Use Cases

- Teams already using multiple model providers and wanting one operational entry point.
- Products, internal tools, or developer platforms that need controlled model access without sharing upstream provider keys directly.
- Teams that need to deploy a model access layer in their own environment while keeping keys, usage, and routing policies under control.
- AI applications, automation tools, and internal model services that need a lightweight gateway foundation.

## 3. Quick Start

### 1. Prepare PostgreSQL

NeoGate requires PostgreSQL 16 or a compatible version.

```bash
createdb neogate
```

### 2. Start the Backend

```bash
cp backend/.env.example backend/.env
```

Edit `backend/.env` and confirm at least these settings:

```dotenv
DATABASE_URL=postgres://localhost/neogate
PUBLIC_BASE_URL=https://neogate.example.com
ADMIN_TOKEN_SECRET=change-me-admin-token-secret-in-production
UPSTREAM_SECRET_KEY=change-me-upstream-secret-key-in-production
```

On first backend startup, NeoGate creates the default admin `admin` / `password` when the `admin` table is empty. Later admin logins use the password hash stored in the database.

Start the backend:

```bash
cd backend
cargo run
```

The backend listens on:

```text
http://127.0.0.1:8080
```

### 3. Start the Frontend

```bash
cd frontend
pnpm install
pnpm dev
```

The local frontend dev server proxies admin requests to the backend. For production builds, set `VITE_NEOGATE_BACKEND_ORIGIN` to the public backend origin.

### 4. Docker

You can also start NeoGate with Docker Compose:

```bash
cp backend/.env.example .env
docker compose up --build
```

When using Docker Compose, uncomment and set `POSTGRES_PASSWORD` in `.env` first. Replace default secrets before using it in production.

## 4. Deployment Modes

NeoGate can run as a single-node deployment or a clustered deployment. For most teams, single-node deployment is enough to start with: it does not require Redis, keeps configuration simple, and is easier to operate and troubleshoot.

- Single-node deployment: the default mode. No `RUNTIME_MODE` configuration is required. Suitable for personal projects, small teams, and early production deployments.
- Clustered deployment: set `RUNTIME_MODE=distributed`, where multiple backend replicas share Redis. Use this when you clearly need multiple replicas and horizontal scaling.

If you do not clearly need multiple backend replicas, prefer single-node deployment first.

## 5. Production Checklist

Before going live:

- Set `APP_ENV=production`.
- Replace the default admin password after first startup.
- Use long, random values for `ADMIN_TOKEN_SECRET` and `UPSTREAM_SECRET_KEY`.
- Set a trusted `PUBLIC_BASE_URL` for password reset links.
- If the frontend and API are accessed cross-origin, set the correct `CORS_ALLOWED_ORIGINS`; same-origin reverse proxy deployments usually do not need extra CORS configuration.
- Configure SMTP in the admin settings if you want public email-based API key claims.
- For clustered deployment, set `RUNTIME_MODE=distributed` and configure Redis. Otherwise, keep the default single-node mode.

## 6. License

NeoGate Community Edition is licensed under the [GNU Affero General Public License v3.0](LICENSE) (`AGPL-3.0-only`).

NeoGate also offers tiered commercial licensing: ordinary companies may request a free written Internal Commercial License for internal use; customer delivery, managed service, SaaS, OEM, MSP, white-label, resale, and similar third-party use require a separate Commercial License; Enterprise Edition features are governed by a commercial EULA. See [LICENSING.md](LICENSING.md).

The NeoGate name, logo, and related marks are not licensed under AGPL. Their use is governed by [TRADEMARKS.md](TRADEMARKS.md).

## 7. Help

- Issues: [GitHub Issues](https://github.com/neogate-io/NeoGate/issues)
- Contributions: [Pull Requests](https://github.com/neogate-io/NeoGate/pulls)
