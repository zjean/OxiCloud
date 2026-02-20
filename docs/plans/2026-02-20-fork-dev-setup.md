# Fork Development Setup — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Set up zjean/OxiCloud as a proper fork with upstream sync, GHCR publishing, local dev environment, and agent-friendly planning docs workflow.

**Architecture:** Additive-only changes on top of upstream. New files for fork-specific needs (GHCR workflow, dev compose, agent instructions). Orphan branch for planning docs. No upstream files modified except 2 lines appended to .gitignore.

**Tech Stack:** Git, GitHub Actions, Docker, PostgreSQL, GHCR

---

### Task 1: Add upstream remote

**Step 1: Add the remote**

Run:
```bash
git remote add upstream https://github.com/DioCrafts/OxiCloud.git
```

**Step 2: Verify remotes**

Run:
```bash
git remote -v
```
Expected output includes:
```
origin    git@github-prive:zjean/OxiCloud.git (fetch)
origin    git@github-prive:zjean/OxiCloud.git (push)
upstream  https://github.com/DioCrafts/OxiCloud.git (fetch)
upstream  https://github.com/DioCrafts/OxiCloud.git (push)
```

**Step 3: Fetch upstream**

Run:
```bash
git fetch upstream
```
Expected: Fetches branches and tags from DioCrafts/OxiCloud.

---

### Task 2: Update .gitignore

**Files:**
- Modify: `.gitignore` (append at end)

**Step 1: Append fork-specific ignores**

Add these lines to the end of `.gitignore`:

```gitignore

# Agent planning docs (live on 'planning' branch)
docs/plans/

# Claude Code artifacts
.claude/
```

**Step 2: Verify**

Run:
```bash
tail -6 .gitignore
```
Expected: Shows the newly added lines.

**Step 3: Commit**

Run:
```bash
git add .gitignore
git commit -m "chore: gitignore agent planning docs and claude artifacts"
```

---

### Task 3: Create CLAUDE.md

**Files:**
- Create: `CLAUDE.md`

**Step 1: Write CLAUDE.md**

Create `CLAUDE.md` in the repo root with this content:

```markdown
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
```

**Step 2: Commit**

Run:
```bash
git add CLAUDE.md
git commit -m "chore: add agent instructions for fork development"
```

---

### Task 4: Create docker-compose.dev.yml

**Files:**
- Create: `docker-compose.dev.yml`

**Step 1: Write the dev compose file**

Create `docker-compose.dev.yml` in the repo root:

```yaml
# Local development: PostgreSQL only.
# Usage: docker compose -f docker-compose.dev.yml up -d
# Then run OxiCloud natively: cargo run

services:
  postgres:
    image: postgres:16-alpine
    ports:
      - "5432:5432"
    environment:
      POSTGRES_USER: oxicloud
      POSTGRES_PASSWORD: oxicloud
      POSTGRES_DB: oxicloud
    volumes:
      - pgdata_dev:/var/lib/postgresql/data
      - ./db/schema.sql:/docker-entrypoint-initdb.d/schema.sql
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U oxicloud"]
      interval: 10s
      timeout: 5s
      retries: 5

volumes:
  pgdata_dev:
```

**Step 2: Verify it starts**

Run:
```bash
docker compose -f docker-compose.dev.yml up -d
```
Expected: PostgreSQL container starts, schema auto-applied.

Run:
```bash
docker compose -f docker-compose.dev.yml ps
```
Expected: postgres service is "running" and healthy.

**Step 3: Tear down**

Run:
```bash
docker compose -f docker-compose.dev.yml down
```

**Step 4: Commit**

Run:
```bash
git add docker-compose.dev.yml
git commit -m "chore: add dev compose for local PostgreSQL"
```

---

### Task 5: Create GHCR publish workflow

**Files:**
- Create: `.github/workflows/ghcr-publish.yml`

**Step 1: Write the workflow**

Create `.github/workflows/ghcr-publish.yml`:

```yaml
name: GHCR Publish

on:
  push:
    branches: [main]
  workflow_dispatch:

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository }}

jobs:
  test:
    name: Pre-publish Tests
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16-alpine
        env:
          POSTGRES_USER: postgres
          POSTGRES_PASSWORD: postgres
          POSTGRES_DB: oxicloud_test
        ports:
          - 5432:5432
        options: >-
          --health-cmd "pg_isready -U postgres"
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Initialize test database
        run: psql -h localhost -U postgres -d oxicloud_test -f db/schema.sql
        env:
          PGPASSWORD: postgres
      - name: Run tests
        run: cargo test --all-features --workspace
        env:
          DATABASE_URL: "postgres://postgres:postgres@localhost/oxicloud_test"

  build-and-push:
    name: Build & Push to GHCR
    runs-on: ubuntu-latest
    timeout-minutes: 120
    needs: test
    permissions:
      contents: read
      packages: write
    steps:
      - name: Checkout
        uses: actions/checkout@v6

      - name: Set up QEMU
        uses: docker/setup-qemu-action@v3

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract metadata
        id: meta
        uses: docker/metadata-action@v5
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}
          tags: |
            type=raw,value=latest
            type=sha,prefix=
            type=raw,value={{date 'YYYY-MM-DD'}}

      - name: Build and push
        uses: docker/build-push-action@v6
        with:
          context: .
          platforms: linux/amd64,linux/arm64
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

**Step 2: Validate YAML syntax**

Run:
```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ghcr-publish.yml'))" && echo "YAML OK"
```
Expected: `YAML OK`

**Step 3: Commit**

Run:
```bash
git add .github/workflows/ghcr-publish.yml
git commit -m "ci: add GHCR publish workflow for fork"
```

---

### Task 6: Create planning orphan branch

**Step 1: Ensure working tree is clean**

Run:
```bash
git status
```
Expected: `nothing to commit, working tree clean`

**Step 2: Create orphan branch**

Run:
```bash
git checkout --orphan planning
git rm -rf .
```

**Step 3: Add README and design doc**

Create a `README.md` on this branch:

```markdown
# OxiCloud Planning Docs

Agent-generated planning documents, designs, and research.
This branch has no shared history with main — it exists purely for documentation.

## Contents

Design docs and implementation plans created during development sessions.
```

Copy the design doc that was created during brainstorming:

```bash
mkdir -p docs/plans
```

Then write `docs/plans/2026-02-20-fork-dev-setup-design.md` with the content from the brainstorming session (already exists locally in the working tree — recreate it here since the orphan branch starts empty).

**Step 4: Commit and push**

Run:
```bash
git add README.md docs/
git commit -m "init: planning branch for agent-generated docs"
git push -u origin planning
```

**Step 5: Return to main**

Run:
```bash
git checkout main
```

**Step 6: Verify**

Run:
```bash
git branch -a
```
Expected: Shows `main`, `planning`, and remote branches.

---

### Task 7: Push main to origin

**Step 1: Push all commits**

Run:
```bash
git push origin main
```

**Step 2: Verify**

Run:
```bash
git log --oneline -6
```
Expected: Shows the 4 new commits (gitignore, CLAUDE.md, dev compose, GHCR workflow) on top of existing history.

---

## Post-Setup Verification Checklist

- [ ] `git remote -v` shows both origin and upstream
- [ ] `git branch` shows main and planning
- [ ] `.gitignore` blocks `docs/plans/` and `.claude/`
- [ ] `CLAUDE.md` exists on main
- [ ] `docker-compose.dev.yml` starts PostgreSQL
- [ ] `.github/workflows/ghcr-publish.yml` exists
- [ ] `planning` branch exists on origin with the design doc
- [ ] `docs/plans/` directory is ignored by git on main
