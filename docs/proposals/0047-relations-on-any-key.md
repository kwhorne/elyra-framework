# RFC 0047 — Relations on any key: a parent named by a string, or by two columns

**Status:** accepted (2026-11-21) · **Target:** 0.52.0 · builds on [models](../models.md), [RFC 0044](0044-composite-keys.md) · see [Decisions](#decisions)

## Summary

Relations key on one `i64` column. A model named by anything else can't take
part in `has_many`, `has_one` or `belongs_to`:

- **A composite-key parent** (`order_lines` keyed by `order_id` and
  `line_no`) is refused with a clear compile error (RFC 0044).
- **A single non-`i64` key** (`teams` keyed by `code: String`) isn't
  refused. It fails inside the generated code: `has_many` from it with
  "cannot move out of a shared reference", and `belongs_to` to it with
  "mismatched types". The models page says only that "relation
  eager-loading still assumes `i64` keys"; the lazy accessors don't
  compile either.

This RFC lets a relation key on the parent's key, whatever it is:

```rust
#[derive(Model)]
#[model(table = "teams", has_many(Member, fk = "team_code", as = "members"))]
struct Team { #[model(id)] code: String, name: String }

#[derive(Model)]
#[model(table = "members", belongs_to(Team, fk = "team_code"))]
struct Member { id: i64, team_code: String, name: String }

#[derive(Model)]
#[model(table = "order_lines", has_many(LineNote, fk = ("order_id", "line_no"), as = "notes"))]
struct OrderLine { #[model(id)] order_id: i64, #[model(id)] line_no: i64, qty: i64 }

#[derive(Model)]
#[model(table = "line_notes", belongs_to(OrderLine, fk = ("order_id", "line_no"), as = "line"))]
struct LineNote { id: i64, order_id: i64, line_no: i64, body: String }

team.members(&db).await?;                                   // Vec<Member>
member.team(&db).await?;                                    // Option<Team>
let by_line = OrderLine::load_notes(&db, &lines).await?;    // HashMap<(i64, i64), Vec<LineNote>>
let notes = &by_line[&line.key()];
note.line(&db).await?;                                      // Option<OrderLine>
```

- **`fk` names as many columns as the parent's key has**, in the order of
  the parent's key: a string for one, a tuple of strings for more.
- **`load_*` maps are keyed by the parent's key type** (`Model::Key`).
  For an `i64` key that's `HashMap<i64, _>`, exactly as today.
- **`Model::key(&self)`** returns a row's key, so a map is looked up with
  `map[&line.key()]`.
- `belongs_to_many` still needs an `i64` parent and related key; its pivot
  holds integers.

## Motivation

- **Schemas the app didn't design**: Laravel apps the desktop half meets
  (RFC 0005), and SQLite files an app inherits, key on codes, slugs, UUIDs
  and pairs. RFC 0044 gave such tables a model. Their relations still have
  to be written as `Query` calls and joined by hand.
- **A compile error from inside generated code is a bug**: the derive
  accepts `has_many` on a `String`-keyed model and then produces code that
  doesn't compile, with no pointer to why.
- **The roadmap's Models line** ends with "a composite-key model as a
  relation's parent". After RFC 0046 it's the last item there.

## Current state (0.51)

- `has_many` and `has_one`: the lazy accessor is
  `C::query().where_eq(fk, self.pk)`. `load_*` goes through
  `eager_has_many::<P, C>`, which collects parent keys with
  `get_i64(parent_pk)` and groups the children by `get_i64(fk)`.
- `belongs_to`: the lazy accessor reads the child's fk with `get_i64` and
  calls `P::find(db, id)`. `load_*` goes through `eager_belongs_to`, keyed by
  `i64`.
- Field relations (`with_*`) fill the field from those maps, looked up by
  `get_i64(PK)`.
- A composite parent of `has_many`, `has_one` or `belongs_to_many` is a
  compile error. A non-`i64` single key isn't checked, and fails as above.

## Design

### Declaring it

`fk = "col"` as today, or `fk = ("a", "b")` with as many columns as the
parent's key, in the parent's key order. The macro checks what it can see:

- **On `belongs_to`**, the fk columns are the declaring model's own, so each
  must be one of its fields (compile error otherwise).
- **On `has_many` and `has_one`**, the count must match the declaring model's
  key (compile error otherwise). The columns belong to the other model, so
  they're checked when the query runs, as today.

A composite key has no default fk: leaving `fk` out on a composite parent,
or on a `belongs_to` to a composite parent, is a compile error that asks for
it. A single key keeps today's default, `{type}_id`.

### The key

`Model` gains `fn key(&self) -> Self::Key`, generated (the key fields,
cloned). The eager paths need `Model::Key: Hash + Eq + Clone`, and need it
decodable from a row through a new `KeyFromRow` trait. `KeyFromRow` is
implemented for `i64`, `i32`, `String` and `bool`, and for tuples of these
of 2 to 4 elements: the scalars the `Any` driver decodes. A key of another
type gets the lazy accessors and a compile error on `load_*` naming
`KeyFromRow`.

### Reading

- **Lazy, from the parent** (`has_many`, `has_one`): one `where_eq` per key
  column, with the parent's key fields cloned. This removes today's move
  error.
- **Lazy, from the child** (`belongs_to`): the child's fk fields, cloned
  into the parent's `Key`, then `P::find`. `Option` fk fields are `None` →
  `Ok(None)`, as a missing `i64` fk is today.
- **Eager, `has_many` and `has_one`**: the parents' keys, then one query:
  `where_in` for one column, or a row-value `IN` for more:
  `(order_id, line_no) IN ((?, ?), (?, ?))`. That works on SQLite (3.15+),
  MySQL and Postgres. The fk columns are selected beside the child's own,
  aliased (`__elyra_key0`, …), and decoded as `P::Key` to group by.
  `get_keyed` becomes the one-column case of this.
- **Eager, `belongs_to`**: the children's fk values, from their fields, then
  the parents with the same `IN`. Keyed by `p.key()`.
- **Field relations** look up by `self.key()` (`has_*`) or the fk fields
  (`belongs_to`), so a `Vec<Member>` field on `Team` hydrates.

### What doesn't change

For an `i64` key every generated signature stays the same: `load_*` returns
`HashMap<i64, _>`, and the SQL for one column is today's. `belongs_to_many`
keeps its `i64` requirement, now with a compile error that names the key
type when the parent's or the related model's key isn't `i64`.

### Testing

`model_servers.rs` on SQLite always, and on MySQL and Postgres in CI: a
`String`-keyed parent and a composite parent, each through lazy and eager
`has_many`, `has_one`, `belongs_to`, the field forms, a child whose fk is
`NULL`, and a batch of parents with no children. Plus a test that the SQL
for an `i64` relation is unchanged. `compile_fail` doctests: an fk tuple of
the wrong length, a `belongs_to` fk column the model doesn't have, a
composite parent without `fk`.

## Implementation plan

1. `elyra-db`: `Model::key`, `KeyFromRow` and its impls, a multi-column
   `IN` clause, `get_keyed` over `K: KeyFromRow`. `eager_has_many`,
   `eager_has_one` and `eager_belongs_to` become generic over the key.
2. `elyra-macros`: the `fk` tuple, the checks, `key()`, the accessors and
   loaders over the key, and the `belongs_to_many` error naming the key.
3. Tests and docs (`models.md`: relations and composite keys), roadmap,
   CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **`fk = ("a", "b")` or `fk = "a, b"`?** A tuple reads as "several", and
   a string list is one more thing to split and validate.
   *Recommendation:* the tuple. `fk = "a"` stays as it is.
2. **`load_*` keyed by `Model::Key`, or new `load_*_by_key` methods beside
   the `i64` ones?** For an `i64` key the two are the same type.
   *Recommendation:* `Model::Key`. Nothing changes for `i64`, and other key
   types don't compile today, so nothing breaks.
3. **`Model::key` as a required trait method**, generated by the derive?
   A hand-written `impl Model` would have to add it.
   *Recommendation:* yes, and say so under Upgrading. The trait's docs
   already describe it as generated, and `Key` came in 0.51.
4. **Very large batches**: the `IN` list isn't chunked today (SQLite allows
   32766 bound values, Postgres 65535). A row-value `IN` uses two or more
   per parent.
   *Recommendation:* chunk the eager queries at 1000 parents. That's the
   same for one column and many, and below every driver's limit.

## Not in this RFC

- **`belongs_to_many` with non-`i64` keys**: the pivot's columns, its
  `attach` and `sync`, and RFC 0046's pivot rows all hold `i64`.
- **Polymorphic relations** (`morphTo`, `morphMany`).
- **`has_many_through`**.
- **`make:resource` for a non-`i64` or composite model.**

## Alternatives considered

- **Keep `i64` maps and add a parallel API for other keys**: two names for
  one thing. Every caller with an `i64` key is unaffected either way.
- **Group eager children in Rust by reading their fields** instead of
  selecting the fk columns aliased: the parent's derive can't see the
  child's fields, so it would need a generated by-name getter for every
  type. The alias costs one extra column per key column in the `SELECT`.
- **`OR` of `AND`s** instead of a row-value `IN`: portable to any SQL, but
  longer, and slower to plan on large batches. All three drivers support
  row values.

## Decisions

1. **`fk = ("a", "b")`**, a tuple of strings, for a key of more than one
   column; `fk = "a"` stays as it is.
2. **`load_*` maps are keyed by `Model::Key`**: unchanged for `i64`, and
   other key types didn't compile before.
3. **`Model::key(&self)` is a required trait method**, generated by the
   derive; Upgrading says a hand-written `impl Model` adds it.
4. **The eager queries are chunked at 1000 parents**, for one key column
   and for many alike.
