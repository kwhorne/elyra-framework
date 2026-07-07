# Database

Feature-gated behind `database`. One `Database` handle spans **SQLite, MySQL,
and Postgres** via sqlx's `Any` driver — the backend is chosen by the URL
scheme. It lives in the GUI-free `elyra-db` crate so the CLI can use it too.

```toml
elyra = { version = "0.1", features = ["database"] }
```

## Connecting

The easiest path is `App::database`, which connects lazily and binds a
`Database` into the container:

```rust
App::new()
    .database("sqlite://app.db?mode=rwc")   // scheme picks the driver
    .commands(commands![list_todos]);
```

| Scheme | Driver |
|---|---|
| `sqlite:` | SQLite |
| `mysql:` / `mariadb:` | MySQL |
| `postgres:` / `postgresql:` | Postgres |

Resolve it in commands:

```rust
use elyra::{Database, Ctx};

#[command]
async fn count(ctx: Ctx) -> Result<i64, String> {
    let db = ctx.get::<Database>();
    let row = elyra::db::sqlx::query("SELECT COUNT(*) AS n FROM todos")
        .fetch_one(db.pool()).await.map_err(|e| e.to_string())?;
    Ok(elyra::db::sqlx::Row::try_get(&row, "n").map_err(|e| e.to_string())?)
}
```

## API

- `Database::connect(url).await` — connect eagerly (async; for the CLI / setup).
- `Database::connect_lazy(url)` — build the pool without connecting; connections
  open on first use (sync; used by `App::database`).
- `db.pool()` — the underlying `sqlx::AnyPool`, for running queries.
- `db.driver()` — the detected `Driver` (`Sqlite` / `MySql` / `Postgres`).
- `db.migrator(dir)` — a [`Migrator`](migrations.md) sharing this pool.

## Writing queries

`sqlx` is re-exported as `elyra::db::sqlx`, so app crates don't need a direct
dependency:

```rust
use elyra::db::sqlx::{self, Row};
```

Placeholders differ per backend (`?` for sqlite/mysql, `$1` for postgres); the
`Any` driver does **not** translate them. For hand-written queries, mind the
target driver — or use [models](models.md), whose query builder renders
placeholders per driver for you.

## Testing status

SQLite is fully test-covered. MySQL and Postgres compile and are supported, but
aren't server-tested in this repo.

## Related

- [Migrations](migrations.md) · [Models](models.md)
- [Configuration — `[database]`](configuration.md)
