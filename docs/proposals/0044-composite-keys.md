# RFC 0044 — Composite keys: a model whose row is named by more than one column

**Status:** accepted (2026-11-21) · **Target:** 0.51.0 · builds on [models](../models.md) · see [Decisions](#decisions)

## Summary

A model's row is named by one column today: an `i64` the database assigns,
or one value the app supplies (`#[model(id)] key: String`). Plenty of
tables are named by two: an order's lines (`order_id`, `line_no`), a
user's settings per workspace (`user_id`, `workspace_id`, `key`), a
pivot with columns of its own (`role_user` with `granted_by` and
`expires_at`). The models page says they aren't supported; such a table is
read with `Query` and written with raw SQL.

This RFC lets `#[model(id)]` mark more than one field:

```rust
#[derive(Model)]
#[model(table = "order_lines", timestamps)]
struct OrderLine {
    #[model(id)]
    order_id: i64,
    #[model(id)]
    line_no: i64,
    product: String,
    qty: i64,
}

let line = OrderLine::find(&db, (42, 3)).await?;          // the key, in field order
line.update(&db).await?;                                   // WHERE order_id = ? AND line_no = ?
line.delete(&db).await?;
OrderLine { order_id: 42, line_no: 4, ..line }.insert(&db).await?;
OrderLine::query().with_trashed().find(&db, (42, 3)).await?;
```

- **The key is a tuple** of the key fields' types, in the order they're
  declared.
- **Every key is the app's**: it's inserted as written and never read
  back, as a single app-supplied key is today.
- **`save()` writes the row whether or not it's there** (an upsert on the
  key), since there's no "unsaved" value to tell.
- What reads and writes a whole table (queries, pagination, soft deletes,
  timestamps, casts on the other fields, global scopes, factories, live
  queries) works as it does for any model.

## Motivation

- **Real schemas have them.** Laravel apps the desktop half meets
  (RFC 0005) and SQLite files an app inherits use composite keys for
  line items, per-scope settings, translations (`post_id`, `locale`) and
  pivots with data. Today each is a struct read through `Query` and
  written with `sqlx::query`, without `update`, `delete` or factories.
- **A pivot with columns** (`belongs_to_many` with `granted_by`) is the
  common case: the relation attaches and detaches, and the pivot's own
  columns have no model to read or change them through.
- **It's the last "not supported" on the models page** about keys; the
  roadmap lists it under Models.

## Current state (0.50)

- `Model::PK` is one column. The derive finds a field marked
  `#[model(id)]`, or one whose column is `id`, and generates `find(db,
  key)`, `insert`, `update` and `delete` (`WHERE pk = ?`), and `save()`,
  which inserts when the key is its type's `Default` (`0`, `""`) and
  updates otherwise.
- `Query::find(db, id: impl Into<Value>)` adds `WHERE table.pk = ?`.
- An `i64` key is the database's (`RETURNING`, `last_insert_id`); any other
  is the app's, inserted as written.
- Relations (`has_many`, `has_one`, `belongs_to`, `belongs_to_many`) key on
  `i64` columns; eager loading groups by `i64`.
- A second `#[model(id)]` is ignored: the first one wins, silently.

## Design

### Declaring it

Two or more fields marked `#[model(id)]` make the key composite, in
declaration order. A cast field can't be one (as today). A composite key
is always the app's: none of its columns is read back after an insert,
even an `i64` one, because the database assigns at most one
autoincrement column, and then the key is that column alone.

### The key

`Model` gains `const KEY: &'static [&'static str]`, the key's columns
(one for a single key, so code that walks it doesn't branch), and an
associated `type Key`: the key field's type for a single key, a tuple for
a composite one. `PK` stays the first key column, for code written
against it.

The generated `find(db, key: Self::Key)` and `Query::find(db, key)` take
the key; `Query::find` accepts anything `Into<M::Key>`, so `find(&db,
42)` and `find(&db, "theme")` still compile for the models that have
them. `Query::where_key(key)` adds the key's `WHERE` for the builder's
other uses (`update`, `delete`, `soft_delete` of one row).

### Writing

- **`insert`** writes every column, the key's included, as written.
- **`update`** sets every column but the key's, `WHERE` each key column
  equals the instance's: changing a key field and calling `update` writes
  to the row the new key names, as for a single key today.
- **`delete`** deletes `WHERE` the key.
- **`save()`** is an upsert on the key: SQLite's and Postgres'
  `INSERT … ON CONFLICT (key columns) DO UPDATE SET …`, MySQL's
  `INSERT … ON DUPLICATE KEY UPDATE …`. With `timestamps`,
  `created_at` is set on insert only and `updated_at` on both. It needs
  the key columns to be the table's primary key or a unique index, which
  the migration says (`t.primary(&["order_id", "line_no"])`).

### Relations

- **From** a composite-key model, `belongs_to` works as it does from any
  model: it reads its own foreign-key column (`OrderLine` belongs to
  `Order` through `order_id`).
- **To** one (`has_many(OrderLine)` from a model whose children are
  named by two columns) works too: it's the child's foreign key that
  matters, not its key.
- **A composite-key model as a parent** of `has_many`, `has_one` or
  `belongs_to_many` is a compile error that says so: those relations key
  on one `i64` column of the parent.
- **A pivot with columns** is read and written through its own model
  (`RoleUser` keyed by `role_id`, `user_id`); `belongs_to_many` keeps
  attaching and detaching. Exposing the pivot's columns on the related
  models (Laravel's `withPivot`) isn't in this RFC.

### What doesn't change

Single keys, `i64` or the app's, generate exactly what they do now; `PK`
keeps its meaning; live queries are keyed by table, so a composite key
changes nothing there; codegen sees the struct's fields as before.

### Testing

The model tests on SQLite, and `model_servers.rs` on MySQL and Postgres in
CI: `find`, `insert`, `update`, `delete`, `save` both ways (new row and
existing), `with_trashed().find`, timestamps on upsert, a factory, a
`belongs_to` from the composite model. The compile errors (a cast key
field; `has_many` from a composite parent) get unit tests in the derive,
on the attribute parsing.

## Implementation plan

1. `elyra-db`: `Model::KEY`, `type Key` and its values, `Query::find`
   over `Into<M::Key>`, `Query::where_key`, the upsert per driver.
2. The derive: more than one `#[model(id)]`, the generated `find`,
   `update`, `delete`, `save`, the compile errors.
3. Tests on all three drivers; the models page's "Primary keys" section
   and its "v1 assumptions".

## Not in this RFC

- **`withPivot`**: a pivot's columns on the models `belongs_to_many`
  returns.
- **`make:resource` for a composite-key model**: its routes, views and
  commands assume one `id`.
- **A composite key as a relation's parent** (`has_many` keyed on two
  columns).
- **`first_or_create` / `update_or_create` / `upsert`** on the query
  builder, which the models page also lists as not done; `save()`'s
  upsert is the instance's, not a bulk one.

## Alternatives considered

- **A key struct** (`OrderLineKey { order_id, line_no }`) generated beside
  the model: names each part at the call site, but it's another type per
  model, and a tuple in declaration order reads well enough for two or
  three columns.
- **A surrogate `id` column**: what's recommended for new tables, and no
  help for a schema the app didn't design.
- **`save()` not generated for a composite key** (insert and update
  only): avoids the upsert, but `save()` is what factories and generated
  code call, and an upsert on the key is what "save this row" means when
  there's no unsaved value.

## Decisions

1. **The key is a tuple**, in declaration order; a key struct can come
   later if call sites ask for names.
2. **`save()` is an upsert on the key**, documented as needing a primary
   key or unique index on those columns.
3. **`Query::find` takes `Into<M::Key>`** from 0.51, said under Upgrading:
   `find(&db, 42)` and `find(&db, "theme")` keep compiling; another type
   for the key (an `i32` for an `i64`) doesn't.
4. **A second `#[model(id)]` makes the key composite**, said under
   Upgrading; it was silently ignored before.
