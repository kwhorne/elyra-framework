# Resources — `rata make:resource`

A *resource* is the vertical slice a desktop app needs for each kind of record:
a list you can search and page through, a form with validation, a detail view,
delete with a confirmation — and the Rust commands, tests, model and migration
behind them. `rata make:resource` writes it, typed end to end, using the
framework's own safety rails. The code is yours afterwards: it's the reference
for how you're meant to do this in Elyra, not a runtime you configure.

This guide goes from a fresh project to a working customer list. The CLI
reference is in [docs/cli.md](cli.md#rata-makeresource); the design and its
decisions are in [RFC 0001](proposals/0001-make-resource.md).

## Three tiers

```bash
rata make:resource Customer               # commands, validation, events, tests — for your model
rata make:resource Customer --view        # + the Svelte screens
rata make:resource Customer --generate …  # + the model, migration, factory, seeder
```

Start with `--generate` for a new table; use the first two on a model you've
written yourself.

## 1. A project with a database

```bash
rata new crm && cd crm
```

A resource needs the `database` feature, `serde_json`, and `tokio` for the
tests. `make:resource` checks and prints the exact lines if they're missing —
it doesn't edit `Cargo.toml`:

```toml
[dependencies]
elyra = { …, features = ["database"] }
serde_json = "1"

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## 2. Generate

```bash
rata make:resource Team --generate name:string:unique
rata make:resource Customer --generate \
    name:string email:email:unique 'phone:string?' active:bool=true \
    team_id:references:Team
```

A field is `name:type[:modifier…][?][=default]` — the table of types is in
[the CLI docs](cli.md#--generate). In zsh, quote a field with `?`.

Generate a parent before the resources that reference it: `references:Team`
needs `Team` to exist, and the migrations run in the order you generate them.

## 3. Wire it once

The first resource prints what `main.rs` needs, and it's the last edit you
make there — every later resource is picked up by the registry:

```rust
mod resources;

App::new()
    .database("sqlite://crm.db?mode=rwc")
    .commands(resources::commands())
    .migrations(resources::migrations())
    .seeders(resources::seeders())
    .allow_abilities(resources::abilities())
    // …
```

`allow_abilities` is deliberate: each command is gated by an ability
(`customers.view`, `.create`, `.update`, `.delete`), denied until the app grants
it. Grant fewer to make a read-only app, or generate with `--no-abilities` for
a prototype.

## 4. Test, migrate, seed

```bash
cargo test resources::                   # the generated tests, on throwaway SQLite files
ELYRA_MIGRATE=up cargo run               # create the tables
ELYRA_SEED=1 cargo run                   # 20 teams, 20 customers
```

The tests run the real migrations and cover the whole slice: create, list,
show, update and delete; search, the sort allowlist, paging and its cap;
validation on create *and* update; `unique` (which skips the row being saved)
and `exists`; the domain events; and that every command needs a granted
ability.

## 5. Run it

```bash
rata codegen                              # the typed api.* the views call
rata dev
```

**Customers** and **Teams** are in the nav. `#/customers` lists them,
`#/customers/new` creates one, `#/customers/12` shows one and
`#/customers/12/edit` edits it.

## What you got

```
src/resources/mod.rs                     the registry — rata's, regenerated
src/resources/customer/
    mod.rs                               what the resource contributes
    model.rs                             #[derive(Model)] Customer + impl Factory
    migration.rs                         CreateCustomersTable
    seeder.rs                            CustomerSeeder
    commands.rs                          the five commands, input/query types, events
    tests.rs                             TestApp tests
app/src/resources/index.js               the frontend registry — rata's, regenerated
app/src/resources/customers/
    index.js                             routes + nav entry
    Index.svelte  Form.svelte  Show.svelte
```

Everything under a resource's folder is yours to change; the two registries
are rata's (their first line says so, and rata refuses to overwrite one that
doesn't). After deleting a resource folder by hand, `rata resources:sync`
rebuilds them.

### The commands

| Command | Ability | What it does |
|---|---|---|
| `customers_index(query)` | `customers.view` | search over the text columns, sort by an allowlisted column, a page (at most 100 rows) |
| `customers_show(id)` | `customers.view` | one record, or "customer 12 not found" |
| `customers_store(input)` | `customers.create` | validate, insert, dispatch `CustomerCreated` |
| `customers_update(id, input)` | `customers.update` | validate (the same rules), update, dispatch `CustomerUpdated` |
| `customers_destroy(id)` | `customers.delete` | delete — a soft delete if the model has `soft_deletes` — and dispatch `CustomerDeleted` |

`CustomerInput` is what a form may set: no key and no timestamps, so a request
can't set them. Its fields are all optional, so a missing one comes back as a
validation message rather than a decode error. Validation messages go through
the app's `Translator` when it has one.

The events are the hook for everything else — an audit log, a notification,
or `App::broadcast::<CustomerUpdated>("customers:updated")` to refresh other
windows:

```rust
App::new().listen(|e: CustomerCreated, ctx: Ctx| async move {
    ctx.get::<AuditLog>().record("customer created", e.customer.id)
});
```

### The views

- **Index** — search once typing pauses, sortable headers, paging, delete
  behind `confirm()`, an empty state.
- **Form** — create and edit; each field's control follows its type, and a
  validation failure lands under its field.
- **Show** — the record, with edit and delete.

With a `lang/en.json`, every label is a `$t("customers.…")` key, and their
English defaults are added to the file (one `"customers"` key; nothing else is
touched). Translate them in your other locales.

## On a model you already have

```bash
rata make:resource Invoice          # finds `#[derive(Model)] struct Invoice` under src/
rata make:resource Invoice --view   # later: add the screens, keeping the commands
```

The model is read with `syn`, so the commands, rules and form match its
fields. It must derive `Default`, `Clone`, `Serialize`, `Deserialize` and
`specta::Type`, and have an `i64` key — rata says which are missing. Without a
migration of its own, the tests create the table from the struct; keep that in
step with your real migration.

## Changing a resource

The generated code is a starting point:

- **Validation** — edit `RULES` (or `rules(id)`) in `commands.rs`; any of the
  [44 rules](validation.md) work.
- **Searchable / sortable columns** — `SEARCHABLE` and `SORTABLE` at the top of
  `commands.rs`. Anything not in `SORTABLE` sorts by the key.
- **Who can do what** — the `can = "…"` on each command, and what the app
  grants.
- **The screens** — plain Svelte; the scaffold's theme variables style them.

Regenerating with `--force` overwrites the resource's files, so do it before
you've changed them, or diff afterwards. A regenerated migration keeps its
version, so a database that ran it doesn't see a new one.

## Related

- [Models](models.md) · [Migrations](migrations.md) · [Validation](validation.md)
- [Security](security.md) — abilities and the IPC surface
- [Frontend runtime](frontend-runtime.md#routing) — the router the views use
- [Testing](testing.md) — `TestApp`
