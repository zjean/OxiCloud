# SQLx Auto-Migrations + Docker First-Run Improvements

## Problem

OxiCloud's current database setup has several pain points:

1. **Monolithic schema.sql** — 808-line file applied atomically, no versioning
2. **Custom SQL parser** — ~120 lines of hand-rolled dollar-quote-aware splitting in `db.rs`
3. **Duplicate init paths** — PostgreSQL `initdb.d` AND app startup both apply schema
4. **No upgrade path** — existing users can't safely migrate between versions
5. **Manual SQL step** — referenced in issue #22, users expect `cargo run --bin migrate`

## Solution

Replace `apply_schema()` + `split_sql_statements()` with **sqlx migrate**, embedded in the binary.

## Design

### Migration files

```
migrations/
  001_initial.sql    ← current db/schema.sql content (idempotent)
```

Future schema changes become `002_xxx.sql`, `003_xxx.sql`, etc.

### Startup flow

```
docker compose up
  ├─ PostgreSQL starts → healthy
  └─ OxiCloud starts
       ├─ entrypoint.sh (permissions, drop privileges)
       ├─ create_database_pools()
       │    ├─ Connect primary pool (with retries)
       │    ├─ sqlx::migrate!().run(&pool)
       │    │    ├─ Creates _sqlx_migrations table if missing
       │    │    ├─ Runs unapplied migrations in order
       │    │    └─ Idempotent on existing DBs (IF NOT EXISTS)
       │    └─ Connect maintenance pool
       └─ Serve on :8086
```

### Files to change

| File | Action |
|------|--------|
| `migrations/001_initial.sql` | Create — copy of `db/schema.sql` |
| `src/infrastructure/db.rs` | Edit — replace `apply_schema()` + `split_sql_statements()` with `sqlx::migrate!()` |
| `docker-compose.yml` | Edit — remove `initdb.d` volume mount |
| `db/schema.sql` | Keep — reference for manual installs |

### Existing installations

The `001_initial.sql` migration uses `IF NOT EXISTS` / `CREATE OR REPLACE` throughout.
On existing databases, sqlx will mark it as applied; no actual schema changes occur.

### What we're NOT doing

- No rollback/revert support
- No CLI subcommand for migrations
- No admin user creation from env vars
- No migration numbering gaps or domain splitting
