# WOPI Integration — Design Document

**Date:** 2026-02-20
**Status:** Approved
**Branch:** `feature/wopi` (upstream-worthy)

## Goal

Add WOPI (Web Application Open Platform Interface) support to OxiCloud so users can edit documents (docx, xlsx, pptx, odt, etc.) directly in Collabora Online or OnlyOffice via an in-app modal or new browser tab.

## Scope

Phases 1+2+3 from the existing `doc/wopi-integration.md` technical report:
- Core WOPI host endpoints (CheckFileInfo, GetFile, PutFile, Lock/Unlock)
- Frontend integration (editor modal, new-tab mode, context menu)
- Docker setup for both Collabora and OnlyOffice

**Out of scope (Phase 4):** Proof key validation, PutRelativeFile (Save As), RenameFile from editor, multi-instance lock storage, co-editing status indicators.

## Architecture

```
Browser → [Edit in Office] → OxiCloud Frontend
    → GET /api/wopi/editor-url (authenticated, returns iframe URL + token)
    → Opens modal with iframe OR new tab with host page

Collabora/OnlyOffice (WOPI Client) → OxiCloud (WOPI Host)
    → GET  /wopi/files/{id}           (CheckFileInfo)
    → GET  /wopi/files/{id}/contents  (GetFile)
    → POST /wopi/files/{id}/contents  (PutFile)
    → POST /wopi/files/{id}           (Lock/Unlock/RefreshLock)
```

**Key decisions:**
- WOPI routes at `/wopi/` (top-level, like WebDAV/CalDAV) — not under `/api`
- WOPI auth via JWT with `scope: "wopi"` claim — reuses existing `jsonwebtoken` crate, no new crypto dependencies
- Feature-flagged via `WopiConfig.enabled` (same pattern as OIDC, trash, sharing)
- In-memory lock store for v1 (single-instance, `HashMap` behind `RwLock`)
- No new database tables needed
- Only new Cargo dependency: `quick-xml` (for discovery XML parsing)

## Backend Components

### Configuration (`src/common/config.rs`)

Add `WopiConfig` to `AppConfig`:

| Env var | Default | Purpose |
|---------|---------|---------|
| `OXICLOUD_WOPI_ENABLED` | `false` | Feature flag |
| `OXICLOUD_WOPI_DISCOVERY_URL` | — | Editor's `/hosting/discovery` endpoint |
| `OXICLOUD_WOPI_SECRET` | (falls back to JWT secret) | Separate WOPI token signing secret |
| `OXICLOUD_WOPI_TOKEN_TTL_SECS` | `86400` | Token lifetime (24h) |
| `OXICLOUD_WOPI_LOCK_TTL_SECS` | `1800` | Lock expiration (30min) |

### New Files

| File | Layer | Purpose |
|------|-------|---------|
| `src/application/ports/wopi_ports.rs` | Ports | Trait interfaces for WOPI services |
| `src/application/services/wopi_token_service.rs` | Application | Generate/validate WOPI-scoped JWTs |
| `src/application/services/wopi_lock_service.rs` | Application | In-memory file lock management |
| `src/infrastructure/services/wopi_discovery_service.rs` | Infrastructure | Fetch, parse, cache discovery XML |
| `src/interfaces/api/handlers/wopi_handler.rs` | Interface | WOPI HTTP endpoint handlers |

### WOPI Token Service

Extends the JWT approach with a WOPI-scoped claims struct:

```rust
struct WopiJwtClaims {
    sub: String,        // user_id
    file_id: String,
    can_write: bool,
    scope: String,      // "wopi"
    exp: i64,
    iat: i64,
}
```

The `scope: "wopi"` claim prevents regular auth tokens from being used as WOPI tokens and vice versa.

### WOPI Lock Service

`Arc<RwLock<HashMap<String, LockEntry>>>` where `LockEntry = { lock_id, expires_at }`. Background task cleans expired locks every 60 seconds.

### WOPI Discovery Service

Fetches XML from editor's `/hosting/discovery`, parses with `quick-xml`, caches `HashMap<extension, Vec<WopiAction>>`. Refreshes every 24 hours or on cache miss.

## WOPI Endpoints

| Method | Path | Operation |
|--------|------|-----------|
| GET | `/wopi/files/{file_id}` | CheckFileInfo — file metadata JSON |
| POST | `/wopi/files/{file_id}` | Lock/Unlock/RefreshLock/GetLock (dispatched by `X-WOPI-Override` header) |
| GET | `/wopi/files/{file_id}/contents` | GetFile — stream binary content |
| POST | `/wopi/files/{file_id}/contents` | PutFile — save binary content |
| GET | `/api/wopi/editor-url` | Frontend API: returns iframe URL + WOPI token (behind auth middleware) |
| GET | `/wopi/edit/{file_id}` | Host page: server-rendered HTML for new-tab editing |

All `/wopi/` endpoints use `?access_token=` query param for auth (not the regular auth middleware).

Error codes follow WOPI spec: 401 (invalid token), 404 (file not found), 409 (lock conflict with `X-WOPI-Lock` header), 412 (file too large).

## Frontend

### New file: `static/js/features/files/wopiEditor.js`

- `WopiEditor` class with `openInModal(fileId, fileName)` and `openInTab(fileId, fileName)` methods
- Calls `GET /api/wopi/editor-url` to get iframe URL + token
- Modal mode: full-screen overlay with iframe, form-POSTs access_token to editor
- Tab mode: opens `/wopi/edit/{file_id}?access_token=...` in new browser tab (server-rendered host page)
- Close modal refreshes file list

### Changes to existing files

- `static/js/features/files/inlineViewer.js` — intercept document types, open WOPI editor instead of inline viewer
- `static/js/features/files/contextMenus.js` — add "Edit in Office" (modal) and "Edit in Office (new tab)" context menu items
- `static/index.html` — add `<script>` tag for wopiEditor.js

### No PostMessage integration in v1

The close button and ESC key are sufficient. PostMessage (for deeper UI integration like save status, close events) is a Phase 4 enhancement.

## Docker Setup

### New file: `docker-compose.wopi.yml`

Standalone compose overlay for WOPI development:

```bash
docker compose -f docker-compose.dev.yml -f docker-compose.wopi.yml up -d
```

Contains:
- Collabora CODE on port 9980 (active)
- OnlyOffice Document Server on port 8088 (commented out)

Collabora's `aliasgroup1` points to `host.docker.internal:8085` for macOS local dev (OxiCloud runs natively).

No changes to existing `docker-compose.yml` (upstream's production compose).

## Testing

- **Unit tests:** WopiTokenService (generate, validate, reject expired/wrong scope), WopiLockService (lock, conflict, refresh, expiry), WopiDiscoveryService (parse sample XML)
- **Integration tests:** WOPI handler endpoints (CheckFileInfo JSON shape, GetFile streams, PutFile saves, lock conflicts)
- **Manual E2E:** Open .docx in Collabora, edit, save, verify in OxiCloud
- **WOPI Validator:** Microsoft's open-source compliance test suite for endpoint correctness

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `quick-xml` | latest | Parse WOPI discovery XML |

All other needs (JWT, URL encoding, serialization) are covered by existing dependencies.
