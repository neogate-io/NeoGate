# Contributing to NeoGate

Thanks for helping improve NeoGate. This project is a self-hosted Rust LLM API gateway with a Vue admin console, so the most useful contributions are reproducible bug reports, provider compatibility fixes, deployment improvements, tests, and documentation that lowers the first-run cost.

## Before You Start

- Use the `develop` branch as the base for community development.
- Keep pull requests focused and small enough to review.
- Do not commit secrets, API keys, tokens, customer logs, or private deployment details.
- Prefer issues with a clear reproduction, expected behavior, actual behavior, and environment details.

## Local Setup

See [docs/development.md](docs/development.md) for the local development workflow, environment variables, and verification commands.

## Git Hooks

This repository ships a `pre-commit` hook under `.githooks/` that runs `cargo fmt --all -- --check` on staged Rust files, mirroring the CI formatting check. Enable it once per clone:

```bash
git config core.hooksPath .githooks
```

If a commit is blocked, fix with `cargo fmt --all`, then `git add -u` and commit again.

## Verification

Run the checks that match your change:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cd frontend
pnpm install --frozen-lockfile
pnpm lint
pnpm typecheck
pnpm build
```

For Docker image changes, also verify the existing Docker workflow locally where practical:

```bash
docker compose -f docker-compose.build.yml build backend
docker compose -f docker-compose.build.yml build web
```

## Pull Request Checklist

- Explain why the change is needed and what behavior changed.
- Add or update tests for behavior changes when practical.
- Update documentation when commands, routes, deployment, configuration, or user-facing behavior changes.
- Confirm which backend, frontend, or Docker checks you ran.
- Remove generated outputs under `backend/tests/output/` before submitting.

## Good First Contributions

Good first contributions usually include:

- Provider preset or adapter compatibility fixes with a sample request.
- Documentation fixes for quickstart, deployment, or app integration.
- Focused regression tests for relay, billing, routing, or app callbacks.
- Clear error message improvements.
- UI text or form validation fixes that reduce setup confusion.

Larger changes such as new billing behavior, new authentication modes, major routing changes, or new app integration types should start with an issue or design discussion.
