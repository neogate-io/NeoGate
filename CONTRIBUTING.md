# Contributing to NeoGate

Thanks for taking time to improve NeoGate. This project welcomes bug reports, documentation fixes, deployment notes, and focused pull requests.

## Development Setup

Backend:

```bash
cd backend
cargo run
```

Frontend:

```bash
cd frontend
pnpm install
pnpm dev --host 0.0.0.0
```

## Pull Request Guidelines

- Keep changes focused and explain the user-facing behavior.
- Add or update documentation when behavior, configuration, deployment, or APIs change.
- Run relevant backend and frontend checks before opening a PR.
- Do not commit local secrets, upstream API keys, generated runtime config, or private deployment files.

## Useful Areas to Contribute

- Provider integrations and compatibility notes
- Deployment guides and examples
- Usage tracking and billing improvements
- Admin console usability
- Tests, benchmarks, and observability

