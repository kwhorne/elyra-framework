# Migrations

Elyra's `php artisan migrate`. Migrations are SQL files applied in order,
tracked in a `_elyra_migrations` table, and grouped into **batches** so rollback
can undo the most recent `migrate` as a unit.

## File layout

Migrations live in the directory set by `[database].migrations` (default
`migrations/`), named `<version>_<name>.sql` (the "up"), with an optional
`<version>_<name>.down.sql` (the "down", for rollback):

```
migrations/
├── 0001_create_todos.sql
├── 0001_create_todos.down.sql
├── 1751800000_add_users.sql
└── 1751800000_add_users.down.sql
```

`version` is a numeric, sortable prefix. `make:migration` uses a unix timestamp.

```sql
-- 0001_create_todos.sql
CREATE TABLE IF NOT EXISTS todos (
    id    INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    done  INTEGER NOT NULL DEFAULT 0
);
```

```sql
-- 0001_create_todos.down.sql
DROP TABLE todos;
```

## CLI

```bash
rata make:migration create_todos   # scaffold up + down files
rata migrate                       # apply all pending (as one new batch)
rata migrate:status                # list applied/pending
rata migrate:rollback              # undo the most recent batch (runs .down.sql)
```

`rata` connects directly using `[database]` from `elyra.toml` (or `DATABASE_URL`)
— no app binary required.

## Programmatic use

The framework can run migrations at runtime too (e.g. auto-migrate on boot):

```rust
let db = elyra::Database::connect(url).await?;
db.migrator("migrations").run().await?;             // Vec of applied versions
db.migrator("migrations").status().await?;          // Vec<MigrationStatus>
db.migrator("migrations").rollback().await?;         // Vec of rolled-back versions
```

## Portability notes

- The tracking table uses portable types (`VARCHAR`, `INTEGER`, `BIGINT`).
- Migration files run via `sqlx::raw_sql`, so multiple `;`-separated statements
  in one file are supported.
- Migrations run per-statement without a wrapping transaction (so MySQL's
  non-transactional DDL behaves) — write idempotent-friendly SQL.
- Column-type tip: for boolean-ish columns use `INTEGER` (0/1). The `Any` driver
  can't read SQLite's native `BOOLEAN` type; [models](models.md) map `bool`
  fields to `INTEGER`.

## Related

- [Database](database.md) · [Models](models.md)
- [CLI](cli.md) · [Configuration](configuration.md)
