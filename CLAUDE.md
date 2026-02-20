# OxiCloud — Agent Instructions

## Project

Self-hosted cloud storage in Rust (Axum + PostgreSQL). Fork of [DioCrafts/OxiCloud](https://github.com/DioCrafts/OxiCloud).

## Architecture

Clean/hexagonal architecture with 4 layers:
- `src/domain/` — Entities, repository traits, domain errors
- `src/application/` — Use cases, services, DTOs, ports
- `src/infrastructure/` — PostgreSQL repos, filesystem, JWT, caching
- `src/interfaces/` — HTTP handlers, middleware, API routes
- `static/` — Vanilla JS frontend (no framework)

Dependency injection via `AppState` in `src/common/di.rs`. Config in `src/common/config.rs`.

## Git Workflow

- **Remotes:** `origin` = zjean/OxiCloud (fork), `upstream` = DioCrafts/OxiCloud
- **main** stays in sync with upstream. Never commit fork-only junk to main.
- **Feature branches from main:**
  - `feature/*` — upstream-worthy (can be PR'd to DioCrafts)
  - `personal/*` — personal features (never PR to upstream)
- **Conventional commits:** `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`

## Planning Docs

- Write all plans, designs, and research to `docs/plans/` (gitignored on main).
- These are preserved on the `planning` orphan branch.
- NEVER include planning docs in upstream PRs.

## Before Committing

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --workspace
```

## Local Dev

```bash
docker compose -f docker-compose.dev.yml up -d   # PostgreSQL
cargo run                                          # OxiCloud
```

Env vars in `.env` (gitignored):
```
DATABASE_URL=postgres://oxicloud:oxicloud@localhost/oxicloud
STORAGE_PATH=./storage
RUST_LOG=debug
```

## Do NOT commit

- `.env` or any secrets
- `storage/` directory
- `docs/plans/` (gitignored, lives on planning branch)
- Temporary or generated files
