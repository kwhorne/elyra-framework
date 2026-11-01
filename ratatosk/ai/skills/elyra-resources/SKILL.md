---
name: elyra-resources
description: Generate and change Elyra Framework resources with rata make:resource — the commands, validation, abilities, events, tests, Svelte list/form/detail screens, model, migration and seeder for a table, and the resource registries. Use when adding a new kind of record (a CRUD screen) to an Elyra app, or editing a generated resource.
---

# Elyra resources

A resource is the vertical slice for one kind of record. Generate it; don't
hand-write a CRUD screen.

```bash
rata make:resource Team --generate name:string:unique
rata make:resource Customer --generate name:string email:email:unique 'phone:string?' \
    active:bool=true team_id:references:Team
rata make:resource Invoice            # for a #[derive(Model)] you already have
rata make:resource Invoice --view     # add the Svelte screens later
rata make:resource Customer --backend /api/customers --generate name:string email:email   # over a Laravel API
```

- A field is `name:type[:modifier…][?][=default]`; generate a parent before
  what references it. With `--view`, a reference to a parent that has its
  own resource is a searchable `Combobox` in the form and a link by name in
  the detail view, and a `date` field is a `DatePicker`. `--dry-run` shows the files, `--force` regenerates (do
  it before editing, or diff afterwards), `--no-abilities` for a prototype.
- It needs `elyra` with `database`, `serde_json`, and `tokio` for tests; rata
  prints missing lines and never edits `Cargo.toml` or `main.rs`.

## Wire once

```rust
mod resources;

App::new()
    .database("sqlite://crm.db?mode=rwc")
    .commands(resources::commands())
    .migrations(resources::migrations())
    .seeders(resources::seeders())
    .allow_abilities(resources::abilities())
```

Later resources are picked up by the registries (`src/resources/mod.rs`,
`app/src/resources/index.js`), which rata owns: don't edit them; after
deleting a resource folder run `rata resources:sync`.

## What it makes

- `src/resources/customer/`: `model.rs`, `migration.rs`, `seeder.rs`,
  `commands.rs` (`customers_index` and `customers_show` are live;
  `customers_store`, `customers_update`, `customers_destroy` validate and
  dispatch `CustomerCreated` / `Updated` / `Deleted`), `tests.rs`.
- Abilities `customers.view`, `.create`, `.update`, `.delete`.
- `app/src/resources/customers/`: `Index.svelte`, `Form.svelte`,
  `Show.svelte`, routes and the nav entry.

## Changing one

- Validation: `RULES` (or `rules(id)`) in `commands.rs`. Search and sort:
  `SEARCHABLE` and `SORTABLE`.
- Abilities: the `can = "…"` on each command, and what the app grants.
- Links for the assistant's panel: the `link = "/customers/{id}"` on each
  command. Change it if the pages' routes change.
- Afterwards: `cargo test resources::`, `rata migrate`, `rata codegen`.

`search_docs` on the `elyra-framework` MCP server ("make:resource",
"resources") has the field types and flags.
