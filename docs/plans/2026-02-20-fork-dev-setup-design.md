# Fork Development Setup — Design Document

**Date:** 2026-02-20
**Status:** Approved

## Context

This repo (`zjean/OxiCloud`) is a fork of [DioCrafts/OxiCloud](https://github.com/DioCrafts/OxiCloud). The goal is to maintain a local development environment for building personal features, while keeping the repo clean enough to contribute features back upstream.

## Decisions

### Branching Strategy

- **main** stays in sync with upstream. Never diverge intentionally.
- **Feature branches** off main:
  - `feature/*` — upstream-worthy work (can be PR'd to DioCrafts)
  - `personal/*` — personal features (merge to own main, never PR upstream)
- **planning** — orphan branch for agent-generated docs (no shared history with main)

### Git Remotes

| Remote | URL | Purpose |
|--------|-----|---------|
| `origin` | `git@github-prive:zjean/OxiCloud.git` | Your fork |
| `upstream` | `https://github.com/DioCrafts/OxiCloud.git` | Original project |

### Upstream Sync Workflow

```bash
git fetch upstream
git merge upstream/main
git push origin main
```

### Contributing Back to Upstream

1. Branch from main: `git checkout -b feature/my-thing main`
2. Implement (no planning docs, no fork-specific files)
3. Push: `git push -u origin feature/my-thing`
4. Open PR: `zjean/OxiCloud:feature/my-thing` → `DioCrafts/OxiCloud:main`

### Planning Docs (Agent-Generated)

- Written to `docs/plans/` (gitignored on main)
- Preserved on the `planning` orphan branch
- Never included in upstream PRs

### Docker Image Publishing

- **Registry:** GitHub Container Registry (`ghcr.io/zjean/oxicloud`)
- **Triggers:** Push to main + manual workflow_dispatch
- **Platforms:** linux/amd64, linux/arm64
- **Auth:** Built-in `GITHUB_TOKEN` (no extra secrets)
- Existing upstream workflows (Docker Hub, release) left untouched

### Local Dev Environment

- Native Rust + Docker PostgreSQL (`docker-compose.dev.yml`)
- Existing `docker-compose.yml` (upstream's full-stack) left untouched

## Files Added/Modified

| File | Change | Upstream impact |
|------|--------|-----------------|
| `.gitignore` | Append `docs/plans/`, `.claude/` | 2 lines added |
| `CLAUDE.md` | Agent instructions | New file |
| `.github/workflows/ghcr-publish.yml` | GHCR image builds | New file |
| `docker-compose.dev.yml` | Local PostgreSQL for dev | New file |
| `planning` branch | Agent docs storage | Separate branch |
