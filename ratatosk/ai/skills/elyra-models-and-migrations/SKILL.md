---
name: elyra-models-and-migrations
description: Elyra Framework data layer — #[derive(Model)] Active Record, the query builder, relations, casts, factories, SQL and Rust migrations with Schema, seeders and rata migrate. Use when adding a table or column, writing queries, or changing models in an Elyra app (the database feature).
---

# Elyra models and migrations

Needs `elyra` with the `database` feature and `App::database(url)`
(`sqlite://app.db?mode=rwc`, `mysql://…`, `postgres://…`).

## A model

```rust
#[derive(elyra::Model, Default, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[model(table = "posts", timestamps, soft_deletes, belongs_to(User, fk = "user_id"))]
struct Post {
    id: i64,                                   // i64 autoincrement; 0 = unsaved
    user_id: i64,
    title: String,
    published: bool,                           // INTEGER 0/1 column
    #[model(cast = "json")] tags: Vec<String>, // TEXT, JSON
    created_at: i64,                           // unix seconds
    updated_at: i64,
    deleted_at: Option<i64>,
}
```

- `Post::find(&db, 1)`, `Post::all(&db)`, `post.insert(&db)` (sets `id`),
  `save` (insert when `id == 0`), `update`, `delete` (soft with
  `soft_deletes`).
- `Post::query().where_eq("published", true).where_like("title", "%x%")
  .order_by_desc("created_at").limit(20).get(&db)`, `.first`, `.count`,
  `.exists`, `.paginate(&db, page, per_page)` (a `Page<Post>` a command can
  return), `.join(..)`, bulk `.update(&db, &[("col", v.into())])` / `.delete`.
- Relations: `has_many`, `has_one`, `belongs_to`, `belongs_to_many` on the
  struct (lazy accessor + `load_<name>` for a batch) or on a field
  (`with_<field>` hydrates). Avoid N+1: load in batches.
- Unknown `#[model(..)]` options are compile errors; identifiers are
  validated and values bound, so never format values into SQL.
- `impl Factory for Post { fn definition(n: u64) -> Self { .. } }` then
  `Post::factory().count(3).create(&db)`.

## Migrations

- SQL: `rata make:migration create_posts` writes
  `migrations/<version>_create_posts.sql` and `.down.sql`.
- Rust: `rata make:migration create_posts --rust` writes a `RustMigration`
  under `src/migrations/` using `Schema`, registered with
  `App::migrations(vec![Box::new(..)])`:

```rust
Schema::create("posts", |t| {
    t.id();
    t.foreign_id("user_id", "users").on_delete_cascade();
    t.string("title");
    t.boolean("published").default_value("0");
    t.text("tags");
    t.timestamps();
    t.soft_deletes();
    t.index("title");
})
.to_sql(driver)
```

- `Schema::table("posts", |t| { t.add_string("slug").nullable(); })` alters;
  `drop_column`, `rename_column`, `index`, `unique_index`.
- `rata migrate`, `rata migrate:status`, `rata migrate:rollback`. Never edit a
  migration that has run: add another. Migrations aren't wrapped in a
  transaction, so keep each one small.
- Seeders implement `Seeder` and run with `ELYRA_SEED=1 cargo run`.

## Raw SQL

`elyra::db::sqlx::query(..).execute(db.pool())` is there for what the builder
can't say; afterwards `db.touch("posts")` so live queries re-run.

Check the real schema with `database_schema` and `migration_status` on the
`elyra-framework` MCP server before writing a migration, and `search_docs`
("models", "migrations", "schema builder") for the rest.
