# RFC 0046 — Pivot columns: what a many-to-many row says about itself

**Status:** implemented (2026-11-21) · **Target:** 0.52.0 · builds on [models](../models.md), [RFC 0044](0044-composite-keys.md) · see [Decisions](#decisions)

## Summary

`belongs_to_many` attaches, detaches, syncs and eager-loads, but the
pivot row is only two keys. Real pivots say more: who granted a role
and when it expires, a product's quantity in a cart, a member's place in
a team. Today those columns can't be read through the relation, and
`attach` can't write them (a `NOT NULL` column without a default makes
`attach` fail outright).

Since 0.51 a pivot table can be a model of its own: two `#[model(id)]`
fields make a composite key (RFC 0044). This RFC lets the relation name
that model with `using`, and reads each related row together with its
pivot row:

```rust
#[derive(Model, Default)]
#[model(table = "role_user", timestamps)]
struct RoleUser {
    #[model(id)]
    user_id: i64,
    #[model(id)]
    role_id: i64,
    granted_by: Option<i64>,
    expires_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Model)]
#[model(table = "users", belongs_to_many(Role, using = RoleUser))]
struct User { id: i64, name: String }

let roles: Vec<(Role, RoleUser)> = user.roles_with_pivot(&db).await?;
let by_user = User::load_roles_with_pivot(&db, &users).await?; // HashMap<i64, Vec<(Role, RoleUser)>>

user.attach_roles_with(&db, [(admin.id, RoleUser { granted_by: Some(me.id), ..RoleUser::default() })]).await?;
user.update_role_pivot(&db, admin.id, |p| p.expires_at = Some(tomorrow)).await?;
```

- **The pivot is an ordinary model**: `RoleUser::find(&db, (user.id, admin.id))`,
  `save`, `delete`, factories and casts work on it as on any composite-key
  model, with no new pivot type to learn.
- **What exists keeps working**: `roles()`, `roles_query()`, `attach_roles`,
  `sync_roles` and `load_roles` generate what they do today; `using` adds
  methods beside them.
- **Narrowing on the pivot** needs nothing new: `roles_query()` already
  joins the pivot, so `where_eq("role_user.expires_at", …)` and
  `order_by("role_user.created_at")` work. The docs say so.

## Motivation

- **It's the common case.** Laravel apps the desktop half meets (RFC 0005)
  use `withPivot` and `withTimestamps` on most many-to-many relations
  that carry meaning; an app porting one has the columns and no way to
  read them through the relation.
- **`attach` can't fill a required column.** A pivot with
  `created_at NOT NULL` (what `t.timestamps()` makes) can't be attached
  to at all today; the app writes the pivot with raw SQL.
- **The models page says "not supported yet"**, and the roadmap lists it
  under Models as the next step after composite keys.

## Current state (0.51)

- `Pivot { table, parent_fk, related_fk }` with `query`, `eager`,
  `related_ids`, `attach`, `detach`, `detach_all`, `sync`,
  `sync_without_detaching` (`database/src/model.rs`).
- `attach_on` inserts `(parent_fk, related_fk)` only.
- Eager loading selects the related columns plus the pivot's parent key
  (`get_keyed`), in one query.
- Since RFC 0044 a struct with two `#[model(id)]` fields over the pivot
  table is a working model (`find`, `insert`, `save` as an upsert,
  `delete`), but nothing connects it to the relation.

## Design

### Declaring it

`using = T` on `belongs_to_many`, struct or field form. `T` is a model
whose `TABLE` is the pivot (so `pivot = ".."` and `using` together are a
compile error: the table comes from `T`). `fk` and `related_fk` keep
their defaults and overrides; the relation checks once, at first use,
that both are among `T::KEY` and returns `Error::Query` naming the missing
one if not.

### Reading

- `roles_with_pivot(&db) -> Vec<(Role, RoleUser)>`: the related rows in
  the order `roles()` gives them, each with its pivot row.
- `load_roles_with_pivot(&db, &users) -> HashMap<i64, Vec<(Role, RoleUser)>>`.
- **Two queries, not one.** The related rows through the join as today
  (`get_keyed` on the related key), then the pivot rows with
  `RoleUser::query().where_in(fk, parents)`, matched in memory by
  `(parent, related)`. One query would need every column of both tables
  aliased apart (`id`, `created_at` are in both) and a prefixed
  `from_row`; two keep `from_row` as it is and cost one round trip on a
  local database.
- Live queries: both tables are read, so both are dependencies, as the
  join already makes them.

### Writing

- `attach_roles_with(&db, [(id, RoleUser)])`: inserts each pivot row
  through `RoleUser::insert`, with `user_id`/`role_id` overwritten by the
  relation's values (so `..RoleUser::default()` or a factory fills the
  rest), in one transaction. Timestamps are set as for any model.
- `update_role_pivot(&db, role_id, |p| …)`: finds the pivot row, applies
  the closure, `update`s it; `Ok(false)` when it isn't attached.
  Laravel's `updateExistingPivot`.
- `attach_roles` (ids only) with `using` goes through the model too, so a
  pivot with timestamps gets them: each row is `T::default()` with the two
  keys set. This needs `T: Default`; without `using`, it's the plain
  two-column insert as today.
- `sync_roles_with(&db, [(id, RoleUser)])`: attaches the missing with
  their data, detaches the rest, and **updates** the rows already there
  with the given data. `SyncChanges` gains `updated: Vec<i64>`.
  (Laravel's `sync` with attributes.) See open question 3.

### What doesn't change

A relation without `using` generates exactly what it does today. The
pivot's own key is `(fk, related_fk)` in whatever order `T` declares
them; the relation names columns, not tuple positions. Codegen sees
`RoleUser` as a struct like any other, so a command returning
`Vec<(Role, RoleUser)>` types in the page as `[Role, RoleUser][]`.

### Testing

`framework/tests/model_relations.rs` on SQLite, and `model_servers.rs` on
MySQL and Postgres in CI: `roles_with_pivot` (order, a related row shared
by two parents, a parent with none), `load_roles_with_pivot`,
`attach_roles` filling timestamps, `attach_roles_with`,
`update_role_pivot` (attached and not), `sync_roles_with` (attach, update,
detach in one call), a `using` model whose key misses `fk` (the error).
The compile error for `pivot` with `using` is a `compile_fail` doctest
beside the composite-key ones.

## Implementation plan

1. `elyra-db`: `Pivot::with_pivot::<R, T>` and `eager_with_pivot`,
   `attach_rows`, `update_row`, `sync_rows`, the key check;
   `SyncChanges::updated`.
2. `elyra-macros`: `using = T` on `belongs_to_many`, the generated
   methods (struct and field form), the compile error.
3. Tests as above; `docs/models.md` (the many-to-many section and its
   "not supported yet" line), the roadmap, CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **`using = T` naming a model, or `with_pivot(col, …)` naming
   columns?** A column list (Laravel's `withPivot('granted_by')`) needs
   no struct, but it returns the values untyped (`HashMap<String,
   Value>`), and writing them is untyped too.
   *Recommendation:* `using = T`. A typed struct is what the rest of the
   models API gives, and since 0.51 it costs only the struct.
2. **Should plain `attach_roles` go through `T` when `using` is set?**
   It fills timestamps and defaults, but it requires `T: Default` and
   changes one insert into the model's insert.
   *Recommendation:* yes. Without it, `attach_roles` on a pivot with
   `NOT NULL` timestamps fails, which is the bug this RFC is for; a
   `using` model without `Default` gets a compile error that says to
   derive it.
3. **`sync_roles_with` updating rows that are already attached?** Laravel
   does; it means a sync can write a row that didn't change.
   *Recommendation:* yes, and only rows whose data differ (compared
   field by field before the write), reported in `SyncChanges::updated`.
4. **The field form**: `#[model(belongs_to_many(Role, using = RoleUser))]
   roles: Vec<(Role, RoleUser)>`, hydrated by `with_roles`?
   *Recommendation:* yes, when the field's type is a tuple; a
   `Vec<Role>` field with `using` hydrates the related rows only and still
   gets the `_with` writers.

## Not in this RFC

- **A composite-key model as a relation's parent** (`has_many` keyed on
  two columns): the other half of the roadmap's Models line.
- **`wherePivot` / `orderByPivot` helpers**: `roles_query()` already
  joins the pivot, so the table-qualified column does it.
- **Polymorphic many-to-many** (`morphToMany`).
- **`make:resource` for pivot data.**

## Alternatives considered

- **One query with aliased columns**: one round trip fewer, at the cost of
  a second `from_row` that reads prefixed names, generated for every
  model. Worth it if measurements show the second query matters.
- **A `Pivot` trait** for pivot structs instead of a model: another
  derive and another set of rules, for what a composite-key model already
  is.

## As built

As designed, with these specifics:

- **`PivotRow`**, a trait the derive implements for every composite-key
  model, is what `using = T` needs from `T`: setting a key column, an
  insert on the relation's connection (so `attach_*_with` and `sync_*_with`
  run in one transaction), and an update that writes only when the data
  differ. A `using` model with a single key fails to compile with "`T`
  can't be a `belongs_to_many` pivot model"; one without `Default` with
  "the trait bound `T: Default` is not satisfied", both at the `using`.
- **"Differ" is the database's comparison**: `UPDATE … WHERE <key> AND NOT
  (c1 <=> ? AND …)`, with `IS` on SQLite, `<=>` on MySQL and `IS NOT
  DISTINCT FROM` on Postgres, over the columns that aren't the key or the
  timestamps. `T` needn't be `PartialEq`, a cast column compares as stored,
  and `updated_at` is set only when the row was written.
  `update_<one>_pivot` uses the same update, so an unchanged row isn't
  written there either.
- **The pivot's `fk`/`related_fk` are checked at first use** (`Error::Query`
  naming the missing key column), not at compile time: the derive of one
  model can't see another's key.
- **A related id given twice** in `attach_*_with` or `sync_*_with` keeps its
  first row, as the id-only writers collapse duplicates.
- **`update_<one>_pivot`** is named by the relation without a trailing
  `s`, and keeps the relation's keys whatever the closure does to them.
- **Tests**: `framework/tests/model_servers.rs` runs the same walk-through
  on SQLite (always) and on MySQL 8 and Postgres 16 (in CI): reading,
  eager loading with a shared related row, attaching by ids and with data,
  a narrowed query on a pivot column, `update_role_pivot` attached and not,
  `sync_roles_with` attaching, detaching, leaving and updating (a `NULL`
  against a value included), the field form, and a `using` model whose key
  leaves out `role_id`. The four compile errors are `compile_fail`
  doctests on `elyra::pivot_columns`.

## Decisions

1. **`using = T` names a model**, not a column list: the pivot's columns
   are typed, read and written as a composite-key model's.
2. **Plain `attach_*` goes through `T` when `using` is set**, so
   timestamps and defaults are filled; `T` must implement `Default`, and a
   `using` model without it is a compile error that says so.
3. **`sync_*_with` updates the rows already attached whose data differ**,
   compared field by field before the write, and reports them in
   `SyncChanges::updated`.
4. **The field form hydrates `Vec<(R, T)>`** with `with_*` when the
   field's type is a tuple; a `Vec<R>` field with `using` hydrates the
   related rows only and still gets the `_with` writers.
