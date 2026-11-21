# RFC 0043 — Every write seen: live queries that notice raw SQL and other processes

**Status:** implemented (2026-11-21) · **Target:** 0.51.0 · builds on [RFC 0002](0002-live-queries.md) · see [Decisions](#decisions)

## Summary

A live query re-runs when a write it read from is reported, and today only
the model layer reports: `Customer::create`, `Query::update`, a pivot's
`attach`. A `sqlx::query("UPDATE orders …")`, a migration's data fix, a
sync job in a sidecar, or the user's other copy of the app on the same
SQLite file change the table and nothing re-runs until something else
does. RFC 0002 left that to `db.touch("orders")` by hand and named
database change capture as a later source for the same hub.

This RFC is that source: with it on, the database itself says which
tables changed, however they were changed, and the live queries that read
them re-run.

```rust
App::new()
    .database(Database::connect(url).await?.capture_changes())
```

- **SQLite and MySQL**: a trigger on each table counts its writes in one
  row of `_elyra_changes`, and the app looks at the counts four times a
  second; a count that moved is a table that changed.
- **Postgres**: a statement trigger on each table says the table's name
  with `NOTIFY elyra_changes`, and the app listens.
- **What's reported is a table**, the same key the model layer reports,
  so nothing about live queries, `depends_on` or the frontend changes.

## Motivation

- **Raw SQL is normal.** The models page lists what the model layer can't
  do yet; a report query with a window function, a bulk `UPDATE … FROM`,
  a `DELETE` with a subquery are written with `sqlx::query`. Each needs a
  `db.touch(..)` after it, and a forgotten one is a list that's quietly
  stale until the next model-layer write to that table.
- **Other processes write.** A [sidecar](../sidecar.md) that imports a
  CSV, a CLI the app ships next to itself, a second instance of the
  app on a shared SQLite file, a server's own jobs on a MySQL or Postgres
  database the desktop app reads directly. None of them can call
  `db.touch` in this process.
- **Migrations and seeders** write with raw SQL, and `rata migrate --seed`
  in a terminal is another process: the open windows of a running
  `rata dev` show the old rows.
- **The guarantee gets simpler to state.** Today: "a model-layer write
  re-runs what read its table; anything else, report it". With capture:
  "a write re-runs what read its table".

## Current state (0.50)

- `Database` holds an `AnyPool` (sqlx 0.9's `Any` driver over SQLite,
  MySQL and Postgres) and a `ChangeHub`, a broadcast of changed keys that
  every clone shares. Model-layer writes report their table once durable;
  inside `Database::transaction` they're held and sent on commit.
- `LiveRegistry` subscribes to the hub, coalesces keys for a few
  milliseconds, re-runs each subscription whose read set has one of them,
  and pushes a result only when it changed.
- `db.touch(key)` and `ctx.invalidate(key)` report by hand;
  `ctx.depends_on(key)` declares a read the model layer can't see.
- The `Any` driver has no way to reach SQLite's `update_hook` or a
  Postgres connection's notifications: the per-driver handle isn't exposed
  through it. That's why RFC 0002 deferred this.

## Design

### Turning it on

`Database::capture_changes()` turns it on. When the database is
connected, and again after migrations this `Database` runs (its
`Migrator`, the app's `ELYRA_MIGRATE` mode), the app installs what each
driver needs (below) for every table but its own (`_elyra_*`) and
sqlx's, and starts watching. Migrations run by another process (`rata
migrate` in a terminal) are picked up at the app's next start, or with
`watch_tables()`. `Database::capture_status()` says whether it's on, which
tables are watched, and when it last looked; the inspector shows it.

### SQLite and MySQL: a counter per table

```sql
CREATE TABLE _elyra_changes (table_name VARCHAR(191) PRIMARY KEY, version BIGINT NOT NULL);

-- per table, for INSERT, UPDATE and DELETE (SQLite)
CREATE TRIGGER _elyra_orders_update AFTER UPDATE ON orders BEGIN
  INSERT INTO _elyra_changes VALUES ('orders', 1)
  ON CONFLICT(table_name) DO UPDATE SET version = version + 1;
END;
```

MySQL's is the same with `ON DUPLICATE KEY UPDATE`. The trigger runs in
the writer's transaction, so a rolled-back write moves nothing, and a
committed one is visible to the app's next look whoever wrote it.

The app reads the whole table (`SELECT table_name, version FROM
_elyra_changes`, one row per table that ever changed) every 250 ms on one
of the pool's connections, and reports each table whose version moved.
The first look only remembers; nothing is reported for what changed
before the app started.

- **Why a counter, not a log of changes**: one row per table, so nothing
  grows and nothing has to be pruned; a bulk `UPDATE` of ten thousand rows
  is ten thousand bumps of one row, which SQLite and InnoDB do in the
  writer's transaction without another index to maintain.
- **Why polling**: neither database tells another connection that
  something changed. 250 ms is the delay a write from elsewhere shows up
  with; the app's own model-layer writes still report at once.

### Postgres: NOTIFY

```sql
CREATE FUNCTION _elyra_notify() RETURNS trigger AS $$
BEGIN PERFORM pg_notify('elyra_changes', TG_TABLE_NAME); RETURN NULL; END
$$ LANGUAGE plpgsql;

CREATE TRIGGER _elyra_changes AFTER INSERT OR UPDATE OR DELETE OR TRUNCATE
  ON orders FOR EACH STATEMENT EXECUTE FUNCTION _elyra_notify();
```

A statement trigger: one notification per statement, not per row, and
Postgres sends it on commit and folds duplicates within a transaction.
The app listens on a connection of its own (sqlx's `PgListener`, from the
same URL, outside the `Any` pool). When that connection drops and comes
back, every watched table is reported once, since notifications sent
meanwhile are lost.

### Twice, from the app's own writes

A model-layer write reports its table at once, and the trigger reports it
again at the next look (SQLite, MySQL) or on commit (Postgres). The
second report re-runs the subscriptions that read it once more; their
result is the same, so nothing is pushed to the windows. That costs one
query per affected subscription per write: RFC 0017 measured a local
re-run at about 0.35 ms on SQLite, and the registry's 16 ms batch window
folds the two when they land close together.

### Tables made later

A table created after the app installed the triggers (by raw SQL, or a
migration run by another process) isn't watched until the next install:
the next start, or the next migration this app runs.
`Database::watch_tables()` installs them again on demand.

### What it doesn't change

- The keys are table names, as the model layer's are; `depends_on`,
  `touch`, `invalidate` and keys of the app's own (`settings:theme`) work
  as before.
- Without `capture_changes()`, nothing is installed and nothing is
  polled; the app is as it is in 0.50.
- Over the backend (RFC 0005), live queries re-run on Laravel's broadcasts
  and RFC 0017's record keys, not on a database the app doesn't have.
  Capture is about the app's own database.

### Security

Installing triggers is a schema change in the user's database. The app
only adds `_elyra_changes`, `_elyra_notify()` and triggers named
`_elyra_*`, and only for a database it was told to capture; the triggers
write a table name and a number, never a row's data. `capture_changes()`
on a database another application owns (a server's MySQL the desktop app
reads directly) is the developer's decision, and the docs say what it
adds and how to remove it (`Database::unwatch_tables()`).

### Testing

- Each driver in CI (SQLite in-process; MySQL and Postgres in the
  database job's services): raw `INSERT`/`UPDATE`/`DELETE` re-runs a live
  query; a write from a second pool (another process, as far as the app
  can tell) does too; a rolled-back one doesn't; a table made after start
  is watched after `watch_tables()`.
- The app's own model-layer write re-runs once with a pushed change and
  once without (the unchanged second run isn't pushed).
- Postgres: the listener's connection killed and back reports every
  watched table once.
- `framework/tests/live_reruns.rs` measures the extra re-runs.

## Implementation plan

1. `elyra-db`: `capture_changes()`, the install per driver (triggers,
   `_elyra_changes`, `_elyra_notify()`), `watch_tables()`,
   `unwatch_tables()`, `capture_status()`.
2. The watcher: the 250 ms look for SQLite and MySQL, `PgListener` for
   Postgres, reporting into the existing `ChangeHub`.
3. `App`: the install after the migrate mode's migrations, and the
   inspector's line.
4. Docs: live queries, the database page, the models page's "what the
   model layer can't see".

## Not in this RFC

- **Finer than a table** (rows, or the query's own `WHERE`): the roadmap's
  other live-queries item, still waiting for measurements to ask for it.
- **Capture for the backend's offline copy** (RFC 0007): that copy is
  written only by the app.
- **SQLite's `update_hook` through a driver-specific pool**: see
  Alternatives.

## Alternatives considered

- **SQLite's `update_hook` / `commit_hook`**: in-process only (it doesn't
  see another process), per connection (every pooled connection would need
  it), and unreachable through the `Any` driver; it would mean a second,
  SQLite-specific pool beside the `Any` one.
- **`PRAGMA data_version`** (SQLite): changes when another connection
  commits, cheaply, but says nothing about which table, so every live
  query would re-run on every write from elsewhere. It could gate the
  250 ms look (only read `_elyra_changes` when it moved); that's an
  optimisation inside the same design if the look shows up in profiles.
- **A change log table** (one row per write, pruned): says more (the row's
  key), which only matters for finer invalidation, and grows with every
  bulk write until pruned.
- **Postgres logical replication / MySQL binlog**: exact and complete, but
  a server-side configuration change (`wal_level`, replication grants, a
  binlog format) that a desktop app can't make on someone's database.
- **Keep `touch` by hand**: what 0.50 does. It works for code the app
  writes, and can never work for another process.

## As built

- **`elyra-db`'s `capture` module.** `Database` holds a `Capture` shared by
  every clone, with the pool, the driver, the URL and the change hub.
  `capture_changes()` turns it on and starts the watcher in the current
  tokio runtime (without one, at the first `watch_tables()`);
  `capture_changes_every(..)` is clamped to 50 ms; `watch_tables()`,
  `unwatch_tables()` and `capture_status()` (`on`, `every_ms`, `tables`,
  `last_look_ms`, `error`) are as designed. The watcher holds a weak
  reference: it ends with the last `Database` clone, or when
  `unwatch_tables()` bumps its generation.
- **Installs run one at a time** (an async mutex): the watcher's first and
  a migration's can overlap, and the later one's list of tables is the one
  kept. Tables not watched: `_elyra*`, `sqlite_*`, `_sqlx*` and the
  framework's own (`elyra_backend_cache`, `elyra_outbox`,
  `elyra_outbox_failed`, `elyra_jobs`, `elyra_failed_jobs`); a table of the
  app's whose name starts with `elyra_` is watched.
- **Trigger names** are `_elyra_<table>_<op>`, or `_elyra_<hash>_<op>` (FNV-1a
  of the table's name) past 60 characters, for MySQL's 64. MySQL's are
  dropped and created again (`CREATE TRIGGER IF NOT EXISTS` is 8.0.29+);
  Postgres' likewise (`CREATE OR REPLACE TRIGGER` is 14+).
- **A failed look installs again** before the next one (the counts' table
  may have gone); a failed install is retried every 5 s, and the error is
  in `capture_status()`.
- **Postgres reports every watched table when its listener reconnects**,
  not when it first connects: the first connection hasn't missed anything.
  The listener waits at most a second at a time, to notice the `Database`
  going away.
- **`App::capture_changes()`** turns it on for the database `App::database`
  binds, in the MCP stdio mode, and in the `ELYRA_MIGRATE` mode, so the
  tables the migrations make are watched. `Database::migrator(..)` installs
  again after a run or rollback that did something.
- **`rata new` scaffolds no database**, so decision 1's "turned on for the
  SQLite app it scaffolds" became `make:resource`'s hint for a project
  without the `database` feature: "`.database(url).capture_changes()` on the
  App" (not for `--backend`, where the database is only the registry's).
- **The inspector** asks `/__inspector/capture` and says, under the live
  queries, which tables are watched and when it last looked, or that raw
  SQL isn't seen without `capture_changes()`.
- **Tested** on SQLite always, and on MySQL 8 and Postgres 16 in CI's
  database job (`cargo test -p elyra-db --test capture`): raw SQL, another
  connection's write, a rolled-back one, a table made later and
  `watch_tables()`, a migration's table, `unwatch_tables()`, nothing
  without it, and Postgres' listener killed and back. `live_shell.rs`
  re-runs a live command over the protocol on another connection's raw
  `INSERT`.

## Decisions

1. **Opt-in in 0.51.** `capture_changes()` turns it on for any driver;
   `rata new` turns it on for the SQLite app it scaffolds. The default is
   revisited once it has been used.
2. **250 ms by default**, `capture_changes_every(Duration)` for an app
   that wants it slower, never faster than 50 ms.
3. **The app's own writes are reported twice**, once by the model layer at
   once and once by the trigger; the second re-run's result is the same
   and isn't pushed. Nothing is skipped, so a write from elsewhere to the
   same table is never missed.
4. **Installed on connect and after migrations**, only with
   `capture_changes()`, idempotently (`CREATE TRIGGER IF NOT EXISTS`;
   Postgres' `CREATE OR REPLACE`).
5. **`TRUNCATE` on MySQL fires no trigger**: the docs say so, and
   `db.touch(..)` after one, as today.
