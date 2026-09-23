# Ratatosk — the `rata` CLI

Ratatosk is Elyra's Artisan: the squirrel that carries messages between the Rust
root and the Svelte crown. Most commands read [`elyra.toml`](configuration.md)
from the current directory or any parent.

```
rata <command>
```

| Command | What it does |
|---|---|
| `new <name>` | Scaffold a new workspace + Svelte app |
| `dev` | Start Vite + launch the app against it (HMR) |
| `codegen` | specta → TypeScript types + the typed `api.*` facade |
| `build` | Vite build → embedded assets → release binary |
| `bundle` | Package the release binary (`.app` / `.deb` / portable folder) |
| `migrate` | Apply pending database migrations |
| `migrate:rollback` | Roll back the most recent batch |
| `migrate:status` | Show applied/pending migrations |
| `make:migration <name>` | Scaffold `up`/`down` `.sql` files (`--rust` for a schema-builder migration) |
| `make:command <name>` | Scaffold a `#[command]` handler |
| `make:provider <name>` | Scaffold a `Provider` |
| `make:middleware <name>` | Scaffold a command `Middleware` |
| `make:model <name>` | Scaffold a `#[derive(Model)]` struct |
| `make:resource <Model>` | The commands, validation, events and tests for a model (`--view`: the Svelte screens; `--generate <fields>`: the model and migration too) |
| `resources:sync` | Rebuild the resource registries (after removing a resource) |
| `help` | Show usage |

## `rata new`

```bash
rata new myapp [--elyra <path-to-framework-crate>] [--dir <parent>]
```

- `--elyra <path>` — depend on a local framework checkout (`elyra = { path = .. }`)
  instead of a published version, **and** wire the frontend's `@elyra/runtime`
  to the sibling `runtime/` via a `file:` dependency, so `npm install` + build
  work offline. Handy pre-publish / for contributing. Without it, published
  versions are referenced.
- `--dir <parent>` — where to create the project (default: current directory).

The generated project is its own `[workspace]`, so it builds anywhere.

## `rata dev`

Spawns `npm run dev` in the frontend directory, waits for `:5173`, then runs the
app with `ELYRA_DEV_URL=http://localhost:5173` so the webview loads from Vite for
hot reloading. IPC still targets `elyra://localhost`; CORS headers are added for
that exact origin **only while `ELYRA_DEV_URL` is set** (a production build sends
none — see [security](security.md)). Vite is torn down when the app exits.

## `rata codegen`

Runs the app in codegen mode (`ELYRA_CODEGEN_OUT`), which writes the bindings and
exits before opening a window. Output path comes from `[codegen].out`. See
[codegen](codegen.md).

## `rata build`

1. `npm run build` in the frontend dir (emits `dist/`).
2. `cargo build --release -p <app crate>` (embeds `dist/`).

## `rata bundle`

Builds release, then packages it for the **host platform**. Metadata comes from
`[bundle]` in [`elyra.toml`](configuration.md).

| Host | Output |
|---|---|
| macOS | `target/release/bundle/<Name>.app` with `Info.plist` + `PkgInfo`, ad-hoc code-signed (`codesign -s -`) so it launches locally |
| Linux | `<package>_<version>.deb` (built without `dpkg`) + a portable `.tar.gz`, including the `.desktop` entry and `hicolor` icon |
| Windows | a portable folder with the `.exe`, icon and a `README.txt` |

With `[bundle].deep_link = "myapp"` the macOS `Info.plist` gets
`CFBundleURLTypes` and the Linux `.desktop` entry gets
`MimeType=x-scheme-handler/myapp` — without that, the scheme registered by
`App::deep_link` never reaches the app.

Out of scope (they need per-project certificates and external toolchains): real
Developer ID signing + notarization, MSI (WiX) / NSIS installers, and
AppImage/Flatpak.

### App icon

The bundle generates the native dock/Finder icon: it renders the source image
into `Contents/Resources/AppIcon.icns` (via `sips` + `iconutil`; SVGs are
rasterized at 1024 with `qlmanage`) and sets `CFBundleIconFile`. Point at a
source with `[bundle].icon`, or drop one at a conventional path:

```toml
[bundle]
name = "My App"
icon = "app/public/icon.svg"   # .svg or a raster image (png, …)
```

Auto-detected if `icon` is omitted: `app/public/icon.svg`, `app/public/icon.png`,
`assets/icon.png`, `assets/icon.svg`, `icon.png`, `icon.svg` (scaffolded apps
ship `app/public/icon.svg`, so this works out of the box). Icon generation is
best-effort — if the tooling or a source image is missing, the bundle still
builds with the default icon.

## Migrations

`migrate`, `migrate:rollback`, `migrate:status`, and `make:migration` connect
directly to the database (no app binary needed), reading `[database]` from
`elyra.toml`. See [migrations](migrations.md).

```bash
rata make:migration create_users            # up/down .sql files
rata make:migration create_users --rust     # a RustMigration using the schema builder
rata migrate
rata migrate:status
rata migrate:rollback
```

Rust migrations and seeders live in the app, so they run through the app binary:

```bash
ELYRA_MIGRATE=up   cargo run    # apply Rust migrations
ELYRA_MIGRATE=down cargo run    # roll back the last batch
ELYRA_SEED=1       cargo run    # run registered seeders
```

## Generators (`make:*`)

Artisan-style scaffolding. Each writes one source file under `src/` and prints
the wiring step Rust needs (a `mod` line + registration) — `rata` never rewrites
`main.rs` for you.

```bash
rata make:command greet_user   # -> src/greet_user.rs   (#[command] handler)
rata make:provider Payments    # -> src/payments.rs     (PaymentsProvider)
rata make:middleware Timing    # -> src/timing.rs       (Middleware impl)
rata make:model BlogPost       # -> src/blog_post.rs    (#[derive(Model)], table "blog_posts")
```

Names are normalized: `BlogPost`/`blog post` → file `blog_post.rs`, struct
`BlogPost`; model table names are pluralized (`Category` → `categories`). Existing
files are never overwritten.

## `rata make:resource`

The vertical slice for a model that already exists — the part of a desktop app
you'd otherwise write by hand for every table. [The resources guide](resources.md)
walks through it from a fresh project; the design is
[RFC 0001](proposals/0001-make-resource.md).

```bash
rata make:resource Customer                  # finds `#[derive(Model)] struct Customer` under src/
rata make:resource Customer --dry-run        # list what would be written
rata make:resource Customer --no-abilities   # no `can = …` (prototypes)
rata make:resource Customer --force          # regenerate an existing resource
rata make:resource Customer --view           # + the Svelte views
```

It reads the struct with `syn`, so the output matches its fields, and writes
`src/resources/customer/`:

- **`commands.rs`** — `customers_index` (search, sort, pagination),
  `customers_show`, `customers_store`, `customers_update`, `customers_destroy`
  (a soft delete when the model has `soft_deletes`). Each is gated by an ability
  (`customers.view` / `.create` / `.update` / `.delete`) and returns
  `elyra::Result`, so a validation failure reaches the frontend as a
  `ValidationError`. Also `CustomerInput` — what a form may set: no key and no
  timestamps, every field optional so a missing one is a validation message —
  `CustomerQuery`, rules derived from the field types (`required|string`,
  `nullable|integer`, …), translated through the app's `Translator` when it has
  one, and the domain events `CustomerCreated` / `Updated` / `Deleted`.
- **`tests.rs`** — the slice through `TestApp` on a throwaway SQLite file:
  create → list → show → update → delete, search, the sort allowlist, paging
  and the `per_page` cap, validation on create *and* update, the events, and
  that every command needs a granted ability.
- **`mod.rs`** — what the resource contributes to the [registry](#resource-registries).

The safety rails are in the generated code, not around it: the sort column goes
through an allowlist (anything else sorts by the key), `per_page` is capped at
100, and the abilities are denied until the app grants them.

Everything is checked before anything is written. The model must derive
`Default`, `Clone`, `Serialize`, `Deserialize` and `specta::Type` and have an
`i64` key, and `Cargo.toml` needs `elyra` with `features = ["database"]`,
`serde_json` and a dev-dependency on `tokio` with `macros` — rata prints the
missing lines and stops (it doesn't edit `Cargo.toml`). An existing
`src/resources/customer/` is only replaced with `--force`. The generated files
are run through `rustfmt`.

### `--view`

Adds the screens, in `app/src/resources/customers/`, over the typed `api.*`
(run `rata codegen` after):

- **`Index.svelte`** — a table with a debounced search, sortable headers,
  paging (`1–25 of 31`), delete behind `confirm()`, and an empty state.
- **`Form.svelte`** — create and edit in one component. Each field's control
  follows its type (text, number, checkbox, a JSON textarea for cast fields);
  a validation failure from the command becomes a message under each field.
- **`Show.svelte`** — the record, with edit and delete.
- **`index.js`** — the routes (`/customers`, `/customers/new`,
  `/customers/:id`, `/customers/:id/edit`) and a nav entry, picked up by the
  [frontend registry](#resource-registries).

They use the scaffold's theme variables and `.btn` / `.card`, and only the
runtime's router, `confirm`, `toast` and `validationErrors`. When the project
has `lang/en.json`, every label goes through `$t("customers.…")` and the
English defaults are added to that file as one `"customers"` key — the rest of
the file is left as it was, and a `"customers"` key that's already there is
kept. Without it the labels are plain English.

`--view` on a resource whose Rust half exists keeps that half and adds the
views; `--force` regenerates both.

### `--generate`

Everything, from a field list — the model, its migration, a factory and a
seeder, plus the commands, views and tests above:

```bash
rata make:resource Team --generate name:string:unique
rata make:resource Customer --generate name:string email:email:unique \
    'phone:string?' 'bio:text?' active:bool=true visits:integer:index \
    'meta:json?' team_id:references:Team
```

A field is `name:type[:modifier…][?][=default]`:

| type | Rust | column | rules |
|---|---|---|---|
| `string` | `String` | `string` (255) | `string\|max:255` |
| `text` | `String` | `text` | `string` |
| `email` | `String` | `string` | `email\|max:255` |
| `integer` / `bigint` | `i64` | `big_integer` | `integer` |
| `float` | `f64` | `float` | `numeric` |
| `bool` | `bool` | `boolean` | `boolean` |
| `date` | `String` (ISO) | `string` | `date` |
| `json` | `serde_json::Value`, `cast = "json"` | `text` | — |
| `references:Team` | `i64` + `belongs_to(Team)` | `foreign_id` | `integer\|exists:teams,id` |

- `?` — nullable (`Option<T>`, a `NULL` column, `nullable` instead of
  `required`). **Quote it** in zsh, where `?` is a glob: `'phone:string?'`.
- `:unique` — a unique column, and `unique:customers,email` in the rules —
  skipping the row itself on update.
- `:index` — an index on the column.
- `=value` — the column's default, the factory's value and the form's initial
  one.
- `references:Team` needs `Team` to exist already, and the field to be called
  `team_id`. It adds `belongs_to(Team)` (`customer.team(&db)`), and the seeder
  points every row at an existing team, creating one if there's none.

Nothing is inferred from a field's name: `email:email`, not magic on
`email:string`.

It writes, in `src/resources/customer/`:

- **`model.rs`** — `#[derive(Model)] Customer` with timestamps, and
  `impl Factory` — valid, distinct rows (`customer3@example.com`).
- **`migration.rs`** — a `RustMigration` for the table, reversible. Run it
  with `ELYRA_MIGRATE=up cargo run` (it needs `.database(url)` on the App, or
  `DATABASE_URL`); `=down` rolls it back.
- **`seeder.rs`** — 20 rows from the factory: `ELYRA_SEED=1 cargo run`.

The registry lists the migration and seeder, so they need no wiring beyond
`.migrations(resources::migrations())` and `.seeders(resources::seeders())`.
Migrations and seeders run in the order the resources were generated — a
parent before the resources that reference it. The generated tests run the
real migrations, so the table they test is the one you ship.

`--generate` refuses when the model already exists: drop `--generate` to
build the resource on your model instead. A model it generated itself is only
replaced with `--force`, which keeps the migration's version — a database that
already ran it doesn't see a new one.

## Resource registries

`rata make:resource` ([resources guide](resources.md)) puts each resource in a folder of its own — `src/resources/<name>/` and
`app/src/resources/<plural>/` — and gathers them in two registries that rata
owns and regenerates from the folder listing:

- `src/resources/mod.rs` — `commands()`, `migrations()` and `abilities()` over
  every resource (each resource's `mod.rs` exposes `commands()`,
  `migrations()` and `ABILITIES`);
- `app/src/resources/index.js` — `routes` and `nav` (each resource's
  `index.js` exports them). `rata new` scaffolds it empty, and `routes.js` and
  `App.svelte` already read it.

So `main.rs` is wired **once**, and every later resource is picked up
automatically:

```rust
mod resources;

App::new()
    .commands(resources::commands())
    .migrations(resources::migrations())
    .allow_abilities(resources::abilities())
```

rata checks for those lines (read-only) and prints whichever are missing — it
still never edits `main.rs`. A registry is marked `managed by rata` on its first
line; one without the marker is yours and rata refuses to overwrite it.
`rata resources:sync` rebuilds both registries, e.g. after you delete a
resource folder by hand.
