## Elyra Framework

This is an [Elyra Framework](https://elyracode.com/framework) app: a Rust
backend and a Svelte 5 frontend in one desktop binary, with Laravel's
ergonomics. `rata` (Ratatosk) is its CLI, like Artisan. `elyra.toml` at the
root names the app crate, the frontend folder and the database.

### Ask the project, not your memory

- The `elyra-framework` MCP server (`rata mcp dev`) has `search_docs` (the
  framework's guides, for this version), `app_info` (commands, abilities,
  models, features), `database_schema`, `migration_status`, `last_error`,
  `read_log_entries` and `list_commands`. Use `search_docs` before writing
  framework code you aren't sure of: the API is not Tauri's or Laravel's.
- The skills `elyra-commands`, `elyra-models-and-migrations`,
  `elyra-resources`, `elyra-live-queries` and `elyra-laravel-backend` have the
  details. Load the one for the task.

### Commands

- A command is an `async fn` with `#[command]`, registered with
  `commands![..]` on `App::commands`. Its first parameter is always the
  `Ctx` (`_ctx` when unused). Arguments and the return type implement serde
  and `specta::Type`.
- Return `elyra::Result<T>` when it can fail: `?` turns a `ValidationErrors`
  bag into a `ValidationError` in the frontend, fields intact, and a database
  error into a command error. `elyra::Error::command(msg)` and
  `Error::with_kind(kind, msg)` cover the rest.
- `#[command(can = "posts.delete")]` gates a command behind an ability. The
  webview is denied until the app grants it (`App::allow_ability(..)` /
  `allow_abilities`). Rust callers (tests, providers, jobs) aren't gated.
- `#[command(middleware = ["auth"])]` names middleware registered with
  `App::middleware_alias` / `middleware_group`; an unknown name stops the app
  at startup. Global middleware is `App::middleware(..)`, outermost first.
- `#[command(live)]` makes a command a live query: the frontend subscribes and
  gets a new result whenever what it read changes. A live command only reads.

### Services

- `App::bind(value)` / `bind_as::<dyn Trait>(Arc::new(..))` /
  `bind_lazy(..)` put singletons in the container; commands resolve them with
  `ctx.get::<T>()` (panics if unbound) or `ctx.try_get::<T>()`.
- A `Provider` wires a slice of the app: `register` binds (don't resolve
  anything there), `boot` runs with the full `Ctx`.
- Domain events: `ctx.dispatch(event).await`, listened to with
  `App::listen(|e: E, ctx| async move { .. })`. `App::broadcast::<E>("chan")`
  forwards one to the frontend as a typed channel. Push values to the
  frontend with `ctx.get::<EventBus>().emit("chan", &value)`.
- Tests swap a service for a fake with `App::swap(..)` (`Queue::fake()`,
  `Storage::fake()`, `Dispatcher::fake()`, `Http::fake(..)`, …).

### Data (the `database` feature)

- `#[derive(Model)]` with `#[model(table = "..", timestamps, soft_deletes)]`
  is Active Record over the `Database`: `find`, `all`, `insert`, `save`,
  `update`, `delete`, and `Model::query()` with `where_eq`, `where_in`,
  `order_by`, `limit`, `get`, `first`, `paginate`. Column names are validated,
  values are bound. `bool` fields are `INTEGER` 0/1 columns.
- Migrations are `migrations/<version>_<name>.sql` (+ `.down.sql`), or a
  `RustMigration` built with `Schema` and registered with `App::migrations`.
  Never edit a migration that has run: add a new one.
- Write through the model layer, so live queries see the change. After raw
  SQL, report it: `db.touch("table")` or `ctx.invalidate("table")`.

### Files

- A command that reads or writes a file the frontend names takes an
  `elyra::files::ChosenFile`, never a `String` path: `dialog.open`,
  `dialog.save` and `onFileDrop` return them, and one decodes only from what
  the user chose in this run. `file.path()` / `file.name()`; Rust code grants
  its own files (and tests stand in for the user) with
  `ChosenFile::from_path(..)`, `for_saving(..)` and `folder(..)`.
- `Http` (and `Backend`) send files with `.attach(name, &file)` (multipart,
  streamed from disk) beside `.field(..)` / `.fields(&struct)`, and save an
  answer with `.download_to(..)` / `.download_into(..)`, streamed to disk.

### The frontend

- After changing a command's signature or any type it uses, run
  `rata codegen`: it rewrites the typed `api.*`, `live.*` and channel helpers
  in the bindings file. Don't edit that file by hand.
- Call commands through `api.*` from the bindings; `live.*` gives a store for
  each live command.

### Which `rata` command

- `rata make:resource <Model> --generate <fields>` for a new table with its
  list, form and detail; `rata make:resource <Model>` for a model you have.
  It writes the resource folder and the registries; `main.rs` is wired once.
- `rata make:command|make:model|make:provider|make:middleware <name>` for
  single files; `rata make:migration <name> [--rust]` for a migration.
- `rata migrate`, `migrate:status`, `migrate:rollback`; `ELYRA_SEED=1 cargo run`
  seeds.
- `rata dev` runs the app with hot reload; `rata build` and `rata bundle`
  release it.
- rata never edits `main.rs` or `Cargo.toml`: it prints the lines to add.

### Checks

- `cargo test` runs the app's tests: `TestApp::new(App::new()..)` drives the
  real pipeline without a window (`invoke`, `invoke_ok`,
  `invoke_validation_errors`, `live`, `mcp()`).
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt` before you're
  done.
