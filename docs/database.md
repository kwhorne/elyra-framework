# Database

Feature-gated behind `database`. One `Database` handle spans **SQLite, MySQL,
and Postgres** via sqlx's `Any` driver — the backend is chosen by the URL
scheme. It lives in the GUI-free `elyra-db` crate so the CLI can use it too.

```toml
elyra = { …, features = ["database"] }
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

## Changes

Every `Database` clone shares a change hub: each model-layer write reports its
table once it's durable — the generated `insert` / `update` / `delete` /
`save`, `Query`'s bulk `update` / `delete` / `soft_delete` / `restore` (only
when a row changed), factories, and the pivot operations (`attach`, `detach`,
`sync`, which report the pivot table).

```rust
let mut changes = db.changes().subscribe();
tokio::spawn(async move {
    while let Ok(table) = changes.recv().await {
        // "customers", "role_user", … — or `Err(Lagged)`: assume everything changed
    }
});
```

Without [capture](#every-write-seen), the model layer can't see raw SQL, so
report it by hand — a table, or any other key you want to treat the same way:

```rust
sqlx::query("UPDATE orders SET status = 'shipped'").execute(db.pool()).await?;
db.touch("orders");
db.touch("settings:theme");
```

Inside [`transaction`](#transactions), reports are held and sent on commit; a
rollback sends nothing. This is the data side of live queries
([RFC 0002](proposals/0002-live-queries.md)): `elyra::db::live::track` records
the tables a piece of code reads, so it can be re-run when one of them changes.

### Every write seen

With `capture_changes()`, the database itself says which tables changed,
however they were changed: raw SQL, a migration, a [sidecar](sidecar.md),
another process on the same file or server
([RFC 0043](proposals/0043-every-write-seen.md)).

```rust
App::new()
    .database(sqlite_url(&path))
    .capture_changes()

// or, on a Database of your own
let db = Database::connect(url).await?.capture_changes();
```

- **SQLite and MySQL**: a trigger per table and operation (`_elyra_<table>_insert`,
  `_update`, `_delete`) bumps the table's row in `_elyra_changes`, and the
  app reads the counts every 250 ms (`capture_changes_every(..)`, never under
  50 ms). A count that moved is a table that changed.
- **Postgres**: a statement trigger per table (`_elyra_changes`, calling
  `_elyra_notify()`) says the table with `NOTIFY elyra_changes`, and the app
  listens on a connection of its own. When that connection drops and comes
  back, every watched table is reported once.
- **Installed** for every table there is when the database connects, again
  after each migration this `Database` runs (its `migrator`, the app's
  `ELYRA_MIGRATE` mode), and with `db.watch_tables().await`, for a table made
  later by raw SQL or another process. Not for the framework's own tables
  (`_elyra_*`, `elyra_outbox`, `elyra_jobs`, …).
- **The app's own model-layer writes are reported twice**: at once, and by
  the trigger. The second re-run finds the same result, so nothing is pushed
  to the windows.
- **A rolled-back write says nothing**: the trigger ran in its transaction.
- **`TRUNCATE` on MySQL fires no trigger**; `db.touch(..)` after one.
- `db.capture_status()` says what it's doing (the tables, the last look, the
  last error), and the inspector shows it under the live queries.
  `db.unwatch_tables().await` takes it all out again.

It changes the schema of the database it watches: on one another application
owns (a server's MySQL the desktop app reads directly), that's your
decision. Creating a trigger on MySQL with binary logging on may need the
`TRIGGER` and `SUPER` privileges (or `log_bin_trust_function_creators`).

## Testing status

SQLite is fully test-covered (including the query builder, pagination, joins,
soft deletes and transactions). MySQL and Postgres run model CRUD against real
servers in CI.

## Related

- [Migrations](migrations.md) · [Models](models.md)
- [Configuration — `[database]`](configuration.md)
