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

## The schema builder

Writing per-driver DDL by hand defeats the point of one `Database` across
SQLite/MySQL/Postgres. `Schema` renders it for you:

```rust
use elyra::db::{Driver, Schema};

let statements = Schema::create("users", |t| {
    t.id();                                  // per driver: AUTOINCREMENT / AUTO_INCREMENT / BIGSERIAL
    t.string("email").unique();
    t.string("name").nullable();
    t.integer("age").default_value("0");
    t.boolean("active").default_value("1");  // INTEGER 0/1, matching #[derive(Model)]
    t.foreign_id("team_id", "teams").on_delete_cascade();
    t.timestamps();                          // created_at + updated_at
    t.soft_deletes();                        // nullable deleted_at
    t.index("email");
})
.to_sql(Driver::Sqlite);
```

Also available: `Schema::table(..)` for `ALTER` (`add_string` / `add_integer` /
`drop_column` / `rename_column` / `index` / `unique_index` / `drop_index`),
`Schema::drop`, `drop_if_exists`, `rename`, and column helpers `text`, `float`,
`json`, `blob`, `big_integer`, `timestamp`, `string_len`, `primary(&[..])`.

Run it directly when you need to:

```rust
Schema::create("users", |t| { t.id(); }).execute(&db).await?;
```

## Rust migrations

`RustMigration` pairs the schema builder with the same history table and batch
semantics as the `.sql` files, so you can mix both:

```rust
use elyra::db::{Driver, RustMigration, Schema};

pub struct CreateUsers;

impl RustMigration for CreateUsers {
    fn version(&self) -> &str { "20260801120000" }
    fn name(&self) -> &str { "create_users_table" }

    fn up(&self, driver: Driver) -> Vec<String> {
        Schema::create("users", |t| {
            t.id();
            t.string("email").unique();
            t.timestamps();
        })
        .to_sql(driver)
    }

    fn down(&self, driver: Driver) -> Vec<String> {
        Schema::drop_if_exists("users").to_sql(driver)
    }
}
```

Register them and run without opening a window:

```rust
App::new()
    .database("sqlite://app.db?mode=rwc")
    .migrations(vec![Box::new(CreateUsers)])
    .run()
```

```bash
ELYRA_MIGRATE=up   cargo run    # apply pending Rust migrations
ELYRA_MIGRATE=down cargo run    # roll back the most recent batch
```

`rata make:migration create_users_table --rust` scaffolds the file above under
`src/migrations/` and prints the wiring step.

## Seeders

```rust
use elyra::seeder::{BoxSeed, Seeder};
use elyra::db::Database;

struct DemoData;

impl Seeder for DemoData {
    fn name(&self) -> &str { "DemoData" }

    fn run<'a>(&'a self, db: &'a Database) -> BoxSeed<'a> {
        Box::pin(async move {
            elyra::db::sqlx::query("INSERT INTO tags (name) VALUES ('demo')")
                .execute(db.pool())
                .await?;
            Ok(())
        })
    }
}

App::new().database(url).seeder(DemoData).run()
```

```bash
ELYRA_SEED=1 cargo run
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
- [CLI](cli.md) · [Configuration](configuration.md) · [Testing](testing.md)
