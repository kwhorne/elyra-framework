# RFC 0001 — `rata make:resource`

**Status:** implemented (2026-09-23) · **Target:** 0.8.0 · see [Decisions](#decisions) · the guide: [docs/resources.md](../resources.md)

## Summary

One command that scaffolds a working vertical slice for a model — the part of a
desktop app everyone writes over and over: a list you can search and page
through, a form with validation, a detail view, delete with a confirmation, and
the Rust commands behind them.

```bash
rata make:resource Customer                     # the command layer, for an existing model
rata make:resource Customer --view              # + the Svelte views
rata make:resource Customer --generate \
    name:string email:email:unique phone:string? active:bool=true
                                                # everything: model, migration, factory,
                                                # seeder, commands, views, tests
```

It is Laravel's `make:model -mfsc --resource` and Rails' `scaffold`, adapted to
Elyra's two halves: every layer is typed end to end (a `#[derive(Model)]` →
`#[command]`s → the generated `api.*` → Svelte), and the generated code uses the
framework's own safety rails rather than bypassing them.

## Motivation

Every piece already exists — `#[derive(Model)]`, the schema builder, factories,
`Validator`, abilities, typed codegen, `$t`, `confirm()`/`toast()`. What doesn't
exist is the *assembly*: today a CRUD screen means writing ~10 files by hand and
wiring them in five places, and each app gets it slightly differently wrong
(missing validation on update, no ability on delete, an unpaginated list).

A generator is also the best documentation: the code it emits is the reference
for "how you're meant to do this in Elyra".

## Current state (what the proposal has to work with)

- **`rata make:*`** writes one file and prints the wiring step — it **never edits
  `main.rs`**, deliberately (`ratatosk/src/make.rs`).
- **A new project has no database:** the scaffolded `elyra` dependency enables no
  features, and `elyra.toml` has no `[database]` section.
- **The frontend is a single `App.svelte`:** no router, no layout.
- **Codegen** emits typed `api.*` wrappers and types for every `#[command]`, so
  generated views can be fully typed with no extra work.

## The three tiers

| | Model + migration + factory + seeder | Commands + validation | Views | Tests |
|---|:-:|:-:|:-:|:-:|
| `make:resource Customer` | must exist | ✓ | | ✓ |
| `… --view` | must exist | ✓ | ✓ | ✓ |
| `… --generate [fields]` | ✓ | ✓ | ✓ | ✓ |

- Without `--generate`, the model must already exist; its fields are **read from
  the struct** (parsed with `syn`), so the commands and views match it exactly.
  A missing model is an error that suggests `--generate`.
- `--generate` takes the fields on the command line and implies `--view`.
- Common to all: `--dry-run` (list what would be written), `--force` (overwrite
  generated files), `--no-abilities` (see [Security](#security)).

## Field syntax (`--generate`)

`name:type[:modifier…][?][=default]`, Rails-style:

| Type | Rust | Column | Validation |
|---|---|---|---|
| `string` | `String` | `string` (255) | `string\|max:255` |
| `text` | `String` | `text` | `string` |
| `email` | `String` | `string` | `email\|max:255` |
| `integer` / `bigint` | `i64` | `integer` / `big_integer` | `integer` |
| `float` | `f64` | `float` | `numeric` |
| `bool` | `bool` | `boolean` | `boolean` |
| `date` | `String` (ISO) | `string` | `date` |
| `json` | `serde_json::Value` + `cast = "json"` | `text` | `array` |
| `references:Team` | `i64` + `belongs_to(Team)` | `foreign_id` | `exists:teams,id` |

Modifiers: `?` nullable (`Option<T>`, `nullable`), `:unique` (unique index +
`unique:customers,<field>,{id}` — ignoring the row on update), `:index`,
`=value` default. Every non-nullable field is `required`. Nothing is inferred
from field *names* — `email:email`, not magic on `email:string`.

## What gets generated

For `rata make:resource Customer --generate name:string email:email:unique`:

```
src/resources/mod.rs                       rata-owned registry (see Wiring)
src/resources/customer/mod.rs              re-exports
src/resources/customer/model.rs            #[derive(Model)] Customer + impl Factory
src/resources/customer/commands.rs         the five commands + CustomerInput + CustomerQuery
database/migrations/<ts>_create_customers_table.rs   Schema::create(..), reversible
database/seeders/customer_seeder.rs        Customer::factory().count(20)
app/src/resources/customers/Index.svelte   search, sort, pagination, delete
app/src/resources/customers/Form.svelte    create + edit, field errors
app/src/resources/customers/Show.svelte    detail view
app/src/resources/index.js                 rata-owned route + nav registry
lang/en.json                               merged in: customers.* labels (if lang/ exists)
src/resources/customer/tests.rs            the slice, end to end, through TestApp
```

### The commands

Laravel's resource verbs, one `#[command]` each:

```rust
#[command(can = "customers.view")]
async fn customers_index(ctx: Ctx, query: CustomerQuery) -> Result<Page<Customer>, ValidationErrors>
    // search across string fields (`when_some`), sort by an allowlisted column, paginate

#[command(can = "customers.view")]
async fn customers_show(ctx: Ctx, id: i64) -> Result<Customer, ...>

#[command(can = "customers.create")]
async fn customers_store(ctx: Ctx, input: CustomerInput) -> Result<Customer, ValidationErrors>
    // Validator::validate_with(&db) — the same rules as the column definitions

#[command(can = "customers.update")]
async fn customers_update(ctx: Ctx, id: i64, input: CustomerInput) -> Result<Customer, ValidationErrors>
    // unique rules ignore this row: `unique:customers,email,{id}`

#[command(can = "customers.delete")]
async fn customers_destroy(ctx: Ctx, id: i64) -> Result<(), ...>
    // soft delete if the model has `soft_deletes`
```

`CustomerInput` is a separate struct (no `id`, no timestamps), so a form can
never set them; `CustomerQuery` carries `search`, `sort`, `direction`, `page`,
`per_page` (capped). Both are `specta::Type`, so the forms are typed.

Each command dispatches a domain event (`CustomerCreated`, …) — cheap to emit,
and the natural hook for audit logs or `App::broadcast` to other windows.

### The views

Svelte 5 (runes), styled with the scaffold's CSS variables so they match the
theme, using only what already ships:

- **Index** — a table over `api.customersIndex(..)`, a debounced search box,
  sortable headers, pagination from `Page`, a delete button behind `confirm()`,
  `toast()` on success, an empty state.
- **Form** — one component for create and edit; each field's control follows
  its type; a `ValidationError` from the command becomes per-field messages.
- **Show** — the record with edit/delete actions.
- Labels go through `$t("customers.fields.email")` when the project has
  `lang/` — with English defaults merged into `lang/en.json` — and are plain
  text otherwise.

## Wiring — keeping "never edit `main.rs`"

A resource needs registering in several places (commands, migrations, seeders,
routes, abilities). Printing five manual steps per resource doesn't scale;
editing `main.rs` breaks a principle that exists for good reason.

**Proposal: rata-owned registry files.** `src/resources/mod.rs` and
`app/src/resources/index.js` are **regenerated from the directory listing** on
every `make:resource` (idempotent, a header says they're managed):

```rust
// src/resources/mod.rs — managed by rata; regenerated on `rata make:resource`
pub mod customer;
pub mod invoice;

pub fn commands() -> Vec<Box<dyn elyra::Command>> { /* every resource's commands */ }
pub fn migrations() -> Vec<Box<dyn elyra::db::RustMigration>> { /* … */ }
pub fn abilities() -> Vec<&'static str> { /* every resource's ABILITIES */ }
```

`main.rs` gets **one** manual edit, once, the first time — rata checks
(read-only) and prints it only when it's missing:

```rust
mod resources;
App::new().commands(resources::commands()).migrations(resources::migrations())
```

## Prerequisites the generator checks

- **Database** — `--generate` needs the `database` feature and a
  `[database]` section. Missing → it stops and prints the exact lines (it does
  not edit `Cargo.toml` either).
- **A router** — views are only useful if you can reach them. The scaffold has
  none, so this RFC includes a small one in `@elyra/runtime` (below).

### A minimal router in `@elyra/runtime`

Hash-based, no dependency, ~100 lines:

```js
import { router, link, navigate } from "@elyra/runtime";
// app/src/resources/index.js (generated)
export const routes = {
  "/customers": () => import("./customers/Index.svelte"),
  "/customers/new": () => import("./customers/Form.svelte"),
  "/customers/:id": () => import("./customers/Show.svelte"),
  "/customers/:id/edit": () => import("./customers/Form.svelte"),
};
```

`App.svelte` mounts `<Router {routes} />` once; `rata new` would scaffold that
from 0.8 on. Deep links (`myapp://customers/12`) map onto the same routes.

## Security

The generated code is the reference, so it must be the safe version:

- **Abilities on every command** (`customers.view/create/update/delete`).
  Deny-by-default means the screen 403s until the app grants them — rata prints
  `.allow_abilities(resources::abilities())`. `--no-abilities` opts out for
  prototypes.
- **Validation on both create and update**, including `unique` ignoring the
  current row, and `exists` for references.
- **Sorting through an allowlist** of column names, never a raw string into SQL.
- **`per_page` capped**, so a frontend can't ask for a million rows.

## Testing

`src/resources/customer/tests.rs` exercises the slice through `TestApp`: create → list →
update → delete, a validation failure, the `unique` rule on update, and a 403
without the ability — using the factory and `Store::fake()`.

CI: extend the `rata new` smoke test to run
`make:resource Customer --generate …`, then `cargo test` and the frontend build,
so a broken template fails the build rather than a user.

## Alternatives considered

- **Laravel-style composable flags** (`-m -f -s --views`). More flexible, but
  harder to remember and to test combinations of. The three tiers can gain
  granular flags later without breaking.
- **Editing `main.rs` with `syn`.** Robust for simple files, but a user's
  `main.rs` is arbitrary code; one missed case corrupts it. The registry keeps
  rata's edits to files it owns.
- **Reading fields from the migration or the database** instead of the struct.
  The struct is the source of truth for types and casts; the database lacks
  them.
- **A runtime "admin panel"** (Filament/Nova-style, config instead of code).
  Less code to own, but opaque and hard to customise; generated code is yours
  to change. Could come later on top of the same commands.

## Implementation plan

1. **Router** in `@elyra/runtime` + the scaffold's `App.svelte` using it.
2. **Registry** (`src/resources/mod.rs`, `app/src/resources/index.js`) and the
   one-time `main.rs` check.
3. **`make:resource` base tier** — parse the model with `syn`, generate commands,
   input/query types, validation, tests.
4. **`--view`** — the three Svelte views.
5. **`--generate`** — field parsing, model, migration, factory, seeder, lang keys.
6. **Docs + CI smoke test.**

Each step is a PR on its own; 1–3 are useful before the rest lands.

## Decisions

Settled on 2026-09-23:

1. **Tiers as proposed.** Base = commands for an existing model; `--view` adds
   views; `--generate` does everything. One refinement: `--generate` against a
   model that already exists **refuses** rather than overwriting it, and says to
   drop `--generate` (or pass `--force`) — rata never overwrites a file silently.
2. **Abilities on by default**, deny-by-default, with the grant printed;
   `--no-abilities` opts out.
3. **A router in `@elyra/runtime`** — step 1 of the plan.
4. **One folder per resource** (`src/resources/customer/`,
   `app/src/resources/customers/`).
5. **Domain events by default** (`CustomerCreated`, `CustomerUpdated`,
   `CustomerDeleted`).

Changed while implementing:

- The tests live in the resource (`src/resources/customer/tests.rs`), not
  `tests/`: a scaffolded app is a bin crate, and integration tests can't reach a
  bin crate's modules.
- The ability check in the tests asserts the policy and each command's `can`
  rather than a 403: `TestApp` dispatches Rust-side, where abilities (a limit on
  the webview) don't apply.
- A JSON (`cast`) field is `nullable` in the rules, not `required`: its empty
  value would fail `required`, and a missing one keeps the stored value.
- `--generate` puts the migration and seeder in the resource's folder
  (`migration.rs`, `seeder.rs`) rather than `database/`, and the registry
  gains `seeders()` to list them — a resource stays one folder.
- A `json` field's column is `text` on every driver: the JSON cast binds text,
  which a Postgres `JSONB` column rejects through the `Any` driver.
- A missing `[database]` section doesn't stop `--generate` — the code builds
  and its tests run without one; running the migration needs it, and the docs
  say so.
- The nav entry's label is plain English (edit `index.js` to translate it): a
  `$t` in the layout would load the catalog in apps without translations.
