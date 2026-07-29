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

## Building a SQLite URL from a path

```rust
use elyra_db::sqlite_url;

let path = elyra::system::paths().data.map(|d| format!("{d}/myapp/app.db")).unwrap();
App::new().database(sqlite_url(&path))
```

`format!("sqlite://{}", path.display())` is the obvious thing to write and is
**wrong on Windows**: the drive colon and backslashes don't match the URL grammar,
and SQLite answers `unable to open database file`. `sqlite_url` normalizes the
separators and uses the three-slash absolute form; it appends `?mode=rwc` so the
file is created when missing (`sqlite_url_opts(path, false)` opts out).

## Pool options

Defaults are deliberately desktop-shaped (a small pool and a *bounded* wait, not
sqlx's unlimited acquire):

```rust
use elyra::db::{Database, DatabaseOptions};
use std::time::Duration;

let db = Database::connect_with(
    url,
    DatabaseOptions::default()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5)),
).await?;
```

| Option | Default |
|---|---|
| `max_connections` | 5 |
| `min_connections` | 0 |
| `acquire_timeout` | 10s |
| `idle_timeout` | 10min |
| `max_lifetime` | 30min |

SQLite additionally gets `journal_mode = WAL`, `busy_timeout = 5000` and
`foreign_keys = ON` on connect, so the UI thread and background jobs can share the
file.

## Transactions

```rust
db.transaction(|tx| Box::pin(async move {
    sqlx::query("UPDATE accounts SET balance = balance - 100 WHERE id = 1")
        .execute(&mut **tx).await?;
    sqlx::query("UPDATE accounts SET balance = balance + 100 WHERE id = 2")
        .execute(&mut **tx).await?;
    Ok(())
})).await?;
```

Commits on `Ok`, rolls back on `Err` and returns your error. `db.begin()` gives
you the raw `sqlx::Transaction` when you need manual control.

## Testing status

SQLite is fully test-covered (including the query builder, pagination, joins,
soft deletes and transactions). MySQL and Postgres run model CRUD against real
servers in CI.

## Related

- [Migrations](migrations.md) · [Models](models.md)
- [Configuration — `[database]`](configuration.md)
