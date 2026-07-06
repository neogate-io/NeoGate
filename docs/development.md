# Development Guide

This guide covers the common local workflow for NeoGate contributors.

## Repository Layout

- `backend/`: Rust API server, relay, billing, admin APIs, user APIs, setup flow, and app runtime.
- `scheduler/`: Rust scheduler process for recurring background jobs.
- `frontend/`: Vue admin/user/public console.
- `docs/`: quickstart, deployment, design, and release documentation.
- `backend/tests/`: smoke tests, benchmark helpers, fixtures, and ignored generated output.

## Toolchain

- Rust 1.94 or newer.
- Node.js 20 or newer.
- pnpm 10.25.0, managed by Corepack.
- PostgreSQL for a full local runtime.
- Redis for clustered or background-worker scenarios that need it.

Enable pnpm through Corepack:

```bash
corepack enable
```

## Backend Checks

Run from the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The workspace contains `backend` and `scheduler`, so workspace checks cover both Rust crates.

## Frontend Checks

Run from `frontend/`:

```bash
pnpm install --frozen-lockfile
pnpm lint
pnpm typecheck
pnpm build
```

Use `pnpm dev` for the Vite development server.

## Local Runtime

The fastest full-stack trial path is Docker Compose:

```bash
docker compose up -d
```

Then open `http://127.0.0.1:8080` and complete the first-run setup wizard.

For source development, run PostgreSQL first, configure `backend/.env`, then start the backend and frontend separately. Use `docs/deployment/standalone.md` and `docs/deployment/standalone.zh.md` as the reference for required environment variables.

## Backend Smoke Tests

Python smoke tests call a running NeoGate instance and may call real upstream providers. They are not part of the default CI path.

```bash
cd backend
NEOGATE_API_KEY=your_neogate_api_key \
NEOGATE_BASE_URL=http://127.0.0.1:8080/v1 \
python -m unittest tests.smoke.test_openai_image
```

Generated files are written under `backend/tests/output/` and should not be committed.

## Documentation Changes

Update documentation when a change affects:

- Public API routes or protocol compatibility.
- Deployment commands or environment variables.
- Provider, channel, credential, project, billing, or app configuration.
- First-run setup behavior.
- User-facing screenshots or release notes.

Keep Chinese and English docs aligned when both versions exist.
