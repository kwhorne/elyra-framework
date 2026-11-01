# Elyra Framework

[![CI](https://github.com/kwhorne/elyra-framework/actions/workflows/ci.yml/badge.svg)](https://github.com/kwhorne/elyra-framework/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

A Rust + Svelte 5 framework for hyper-responsive desktop apps. Laravel's
ergonomics — container, providers, middleware, a typed bridge — but **compiled,
binary, and without runtime overhead**.

The CLI is **Ratatosk** (`rata`): the squirrel that runs messages up and down
Yggdrasil, between the Rust root and the Svelte crown.

> 📚 **Documentation: [elyracode.com/framework](https://elyracode.com/framework)**
> (source in [`docs/`](docs/README.md)). This page is just a landing page.

## Highlights

- **Binary IPC** — MessagePack over an `elyra://localhost` custom protocol; no
  JSON in the hot path. ([wire format](docs/wire-format.md))
- **`#[command] async fn`** dispatched through a [middleware](docs/middleware.md)
  pipeline, with a [container](docs/container-and-providers.md) and
  [providers](docs/container-and-providers.md).
- **`EventBus` + `channel()`** — Rust→frontend push, batched per flush over a
  long-poll; the UI thread never blocks. ([events](docs/events.md))
- **Typed bridge** — `rata codegen` turns specta types into TS + a typed
  `api.*` facade. ([codegen](docs/codegen.md))
- **Windows, tray, updater** — [multi-window](docs/windows.md),
  [system tray](docs/tray.md), and an [ed25519-verified updater](docs/updater.md).
- **Desktop components** — [`@elyra/ui`](docs/ui.md): Svelte 5 buttons, menus,
  forms, selects, tables, dialogs, tabs and the window's shell, on one set of
  design tokens, keyboard-first, and aware of the app: a `Table` over a live
  query, a `Form` that puts Laravel's `422` under each field, `can="…"` from
  the user's abilities. `rata make:ui` copies one in to change. Plus the
  runtime's themed [About dialog](docs/about.md),
  [auto-update toast](docs/updater.md), and [dialogs / toasts / ⌘K palette /
  context menu](docs/components.md).
- **Desktop APIs** — native [dialogs, shell-open, clipboard, notifications](docs/system.md),
  [the files the user chose](docs/system.md#a-file-the-user-chose) (a dialog's
  or a drop's, granted so a command can't be handed any other path),
  [window control + file drop](docs/windows.md), [global shortcuts](docs/shortcuts.md),
  [app menu](docs/menu.md), a [settings store](docs/store.md),
  [autostart](docs/autostart.md), and [sidecar processes](docs/sidecar.md).
- **Data** — one [`Database`](docs/database.md) over SQLite/MySQL/Postgres,
  [`rata migrate`](docs/migrations.md), and [`#[derive(Model)]`](docs/models.md)
  Active Record with a query builder and relations.
- **Live queries** — [`#[command(live)]`](docs/live-queries.md): the frontend
  subscribes, Elyra records what the command read and re-runs it when a write
  changes it, and every window watching gets the new result — no polling, no
  reload after save.
- **Resources** — [`rata make:resource Customer --generate …`](docs/resources.md)
  writes the vertical slice: a searchable, paged list, a validated form and a
  detail view in Svelte, the ability-gated commands behind them, tests, and the
  model, migration and seeder.
- **A Laravel backend** — [the desktop half of a Laravel app](docs/backend.md):
  Sanctum sign-in with the token in the keychain, the API's `422`s in your
  forms, its broadcasting into the open windows, working offline, with
  writes that wait in an outbox and are sent once, its notifications in
  the OS, counted on the app's icon, and [files](docs/backend.md#files):
  uploads streamed from disk, downloads streamed to it, with progress, and
  queued offline, and [what each user may do](docs/backend.md#what-the-user-may-do),
  from Laravel's permissions (`backend` feature).
- **Every app an MCP server** — [AI agents](docs/mcp.md) call the commands you
  grant as typed tools, inside the open app or headless, with confirmation in
  the client.
- **An assistant in the app** — [the app's assistant](docs/assistant.md): its
  commands as Claude's tools, held to the signed-in user's abilities, asking
  in the app before what can't be undone, and reaching Claude through Laravel
  so no key ships in the app.
- **Agents that know the framework** — [`rata ai:install`](docs/ai-agents.md)
  gives coding agents the framework's guidelines and skills, and `rata mcp dev`
  answers with the app's commands, models, schema, log and the guides — Laravel
  Boost for the Rust side, with a Composer package for the Laravel half.
- **AI SDK** — a Laravel-inspired [`elyra::ai`](docs/ai.md): agents, tools,
  structured output, images, and embeddings over Anthropic + OpenAI (`ai` feature).
- **Shared facades** — [`Cache`](docs/cache.md), [`Storage`](docs/storage.md), and
  [`Queue`](docs/queue.md) with the same surface as the Askr/Laravel side, over
  local backends — one ecosystem, two worlds.
- **Hardened IPC** — a per-run token, origin-scoped CORS, a
  [capability model](docs/security.md) with deny-by-default sidecar spawn, a
  default CSP, and bounded request bodies.
- **Laravel ergonomics** — [`Log`](docs/logging.md), [`Config`](docs/configuration.md),
  [`Secrets`](docs/secrets.md), [`TestApp`](docs/testing.md), a
  [schema builder](docs/migrations.md) + seeders, and Eloquent-style
  pagination / aggregates / joins / soft deletes / transactions.

## Layout

```
elyra-framework/
├── framework/   # elyra          — App, Container/Ctx, Command, events, shell (tao+wry)
├── macros/      # elyra-macros   — #[command] and #[derive(Model)]
├── database/    # elyra-db       — Database, migrations, models (GUI-free)
├── ratatosk/    # ratatosk       — the `rata` CLI
├── runtime/     # @elyra/runtime — invoke(), channel(), generated api.*
└── example/     # elyra-example  — the demo app / DX benchmark
```

## Quick start

```bash
rata new myapp                 # scaffold (see docs/getting-started.md)
cd myapp
(cd app && npm install && npm run build)
rata codegen                   # generate typed bindings
cargo run                      # launch
```

Or run the bundled demo from this repo — no npm needed (serves a built-in
fallback page):

```bash
cargo run -p elyra-example
```

## A command, end to end

```rust
use elyra::{command, commands, App, Ctx};

#[command]
async fn greet(_ctx: Ctx, name: String) -> String {
    format!("Hello, {name}!")
}

fn main() -> elyra::Result<()> {
    App::new()
        .commands(commands![greet])
        .assets(elyra::asset_resolver::<Assets>())
        .run()
}
```

```ts
import { api } from "./bindings";              // generated by `rata codegen`
const greeting = await api.greet("World");     // (name: string) => Promise<string>
```

## Cargo features (`elyra` crate)

| Feature | Enables |
|---|---|
| `database` | `Database`, `#[derive(Model)]`, migrations, `App::database` |
| `ai` | [`elyra::ai`](docs/ai.md) — agents, tools, structured output, images, embeddings |
| `tray` | `App::tray`, the system tray |
| `updater` | the `updater` module (ed25519) |
| `system` | native dialogs, shell-open, clipboard, notifications, paths |
| `shortcuts` | OS-level global keyboard shortcuts |
| `secrets` | `Secrets` — tokens in the OS keychain |
| `http` | [`Http`](docs/http.md) — an HTTP client, Laravel's facade |
| `backend` | [`Backend`, `Auth`](docs/backend.md) — a Laravel API, Sanctum sign-in (enables `http`, `secrets`) |

```toml
elyra = { version = "0.26.0", features = ["database", "tray", "updater", "system", "shortcuts", "secrets"] }
```

## Status

**v0.26.0** — **presence: who else has a record open**, on the web or the
desktop. The app joins Laravel's presence channels the way Echo does in a
browser, so a browser tab and a desktop window on the same customer see each
other: `Backend::presence(["customers.*"])`, the channel rule in
`routes/channels.php`, and `<Presence channel={`customers.${id}`} />`, which
`make:resource --presence` writes. See
[the backend guide](docs/backend.md#who-else-is-here-presence). In 0.25:
**trying the app offline, picking dates, and finding records
from ⌘K**: the inspector's **Simulate offline** unplugs the backend for the
run, so the offline copy, the outbox, the status bar and broadcasting can be
tried without stopping Laravel. `@elyra/ui` gains a `DatePicker` (typed in
the locale's order or picked in a calendar, an ISO date either way) and
`Avatar` / `AvatarGroup`. The ⌘K palette searches the app's records as you
type, and `make:resource --view` wires up both. See
[the inspector](docs/inspector.md#trying-the-app-offline) and
[searching from ⌘K](docs/components.md#searching-the-apps-records). In 0.24:
**the Elyra Inspector**: a developer's drawer in debug builds
(⌘⌥E / Ctrl+Alt+E). Turn an ability off and the button, the command, the
assistant's tool and an MCP agent all lose it at once; see the window as
the assistant sees it, its live queries and its calls. It only takes away,
and a release build doesn't have it. A `<Table>` selects rows, with bulk
actions, and tells the assistant what's selected; `make:resource` picks a
reference in a `Combobox`; `Kbd` draws ⌘K or Ctrl+K. See
[the inspector](docs/inspector.md). In 0.23: **three more desktop controls in `@elyra/ui`**: a `Combobox`
that searches as you type, from a list or a resource's `_index` command; a
`FileField` for the native file dialog and a file dropped on the window,
which `make:resource` now uses for a `file` field; and a `SplitPane` the
user drags, which keeps its size in the settings store. See
[the reference](docs/ui.md). In 0.22: **the assistant sees the window**: each turn carries the
window's route, its live queries and what the app says is selected, as
names and ids, never rows, so "set her credit limit" on a customer's page
needs no "which customer?". A step in the panel links to the record it
touched (`#[command(link = "/customers/{id}")]`), and `make:resource` writes
it. `@elyra/ui` gains an `OutboxDrawer` for the writes Laravel refused. See
[the guide](docs/assistant.md#what-it-sees-of-the-window). In 0.21:
**`@elyra/ui`, desktop components that know the app**: 31 Svelte 5
components, from buttons, forms and dialogs to a table over a live query
and the window's shell, that hide what the user may not do, show Laravel's
validation errors under their fields and work from the keyboard. One set of
design tokens themes them, the app and the runtime's dialogs.
`make:resource --view` builds its screens from them, and `rata make:ui`
copies one into the app. See [the reference](docs/ui.md). In 0.20: **an
assistant in the app**, the app's own commands as the tools of an assistant
for the signed-in user, held to their abilities, reaching Claude through
Laravel so no key ships in the app ([the guide](docs/assistant.md)). In 0.19: **the current Claude API**, Anthropic's
API version `2026-10-21`, structured outputs, and Claude Sonnet 5.5 as the
default model. In 0.18: **Laravel's abilities on
the desktop**: what the signed-in user may do comes from Laravel (spatie's permissions or the Gate, through
`Route::elyraAbilities()`); a command they may not run is refused before any
request, offline too; the list is kept current when roles change on the
web; and `$can(..)` and the generated screens hide what they can't do. See
[the guide](docs/backend.md#what-the-user-may-do). In 0.17: **files between
the desktop and Laravel**, uploads and downloads streamed, only files the
user chose, queued offline, with progress ([files](docs/backend.md#files)).
In 0.16: **coding agents know the
framework**, `rata ai:install`, `rata mcp dev` and `elyracode/elyra-backend`
([coding agents](docs/ai-agents.md)). In 0.15: **Laravel's
notifications on the desktop**, in the OS or as a toast, the unread count on
the app's icon, read and marked everywhere, and `rata make:notifications`
([the guide](docs/backend.md#laravels-notifications)). In 0.14: **working
offline**, with reads from the last copy and writes that wait in an outbox
and are sent once. In 0.13: **broadcasting**, so a
change made on the web shows in the open windows, over the Pusher protocol
([Askr](https://github.com/kwhorne/askr)'s `--pusher`, Reverb or Pusher).
In 0.12: **a Laravel backend**, the desktop half of an app whose web half is
Laravel. The app signs in with a Sanctum token kept in the OS keychain, calls
the same API as the web, shows Laravel's `422`s in its forms, and
`rata make:resource --backend` generates the screens over the API. In 0.11:
MCP resource templates. In 0.10.x: hardening, and `rata migrate` for Rust
migrations. Building on **v0.10.0**, the **AI agents** release.
Every Elyra app can be an
[MCP server](docs/mcp.md): grant the abilities an agent may use, and the
commands behind them become tools for Claude, Cursor or VS Code, typed from
the same specta types as the frontend's bindings and run through the same
middleware and validation. `myapp --mcp` works inside the open app, so the
windows update as the agent works, or runs headless when it's closed. The
calls you name (a delete, say) wait for the user to confirm in the client, live
commands are resources an agent can watch, and every call is logged,
dispatched as an event and shown in the app. `rata mcp install` prints the
client config.

In 0.9: [live queries](docs/live-queries.md). A `#[command(live)]` is a command
the frontend subscribes to: Elyra re-runs it when a write changes a table it
read, and pushes the new result to every window watching. `rata make:resource`
generates live lists and detail views.

In 0.8.0: [`rata make:resource Customer --generate …`](docs/resources.md) — the
vertical slice for a table, from the model and migration to a searchable list,
a validated form and a detail view — on a
[hash router](docs/frontend-runtime.md#routing) and a registry that keeps
`main.rs` to a one-time edit; translated validation messages and
[typed `$t`](docs/i18n.md#typed-keys).

In 0.7.0: [named middleware](docs/middleware.md#per-command-middleware), a
[scheduler](docs/scheduler.md#clock-times) that runs at clock times and survives
sleep, [validation](docs/validation.md) of nested input, arrays and the database,
and one set of [translation files](docs/i18n.md) for Rust and Svelte.

Earlier, in 0.6.0: `belongs_to_many`, casts, scopes and factories; trait-object
and lazy container bindings; domain events with a typed path to the frontend;
durable queues; and test fakes with `App::swap`.

The rest of the IPC surface was already gated: a random per-run
[token](docs/security.md), CORS only for the dev origin, a
[capability model](docs/security.md) where destructive routes are opt-in, a
default CSP, deny-by-default sidecar spawn, a policy for `shell.open`, and
bounded + depth-checked request bodies. The event bus fans out per window
(multi-window apps no longer lose events), a panicking command answers with an
error instead of hanging the caller, and the settings store writes atomically.

The Laravel-shaped gaps are filled: [`Log`](docs/logging.md),
[`Config`](docs/configuration.md), [`Secrets`](docs/secrets.md) over the OS
keychain, [`TestApp`](docs/testing.md), a [schema builder](docs/migrations.md) with
Rust migrations + seeders, and Eloquent-style pagination / aggregates / joins /
soft deletes / transactions. Plus queue retries + a failed-job list, typed event
channels in [codegen](docs/codegen.md), asset `ETag`/`Range` caching, a
cross-platform app menu, and `.deb` / portable bundling.

Earlier releases brought the core (commands, events, DB/models, codegen, About +
auto-update), native **system integration** (dialogs, shell-open, clipboard,
notifications, paths), **UI components** (dialogs, toasts, ⌘K command palette,
context menu), **window control** + file drop + state persistence, **global
shortcuts**, a native **app menu**, a **settings store**, **autostart**,
**sidecar** processes, **single-instance** + **deep-linking**, a Laravel-inspired
**[AI SDK](docs/ai.md)** (agents, tools, sub-agents, structured output, streaming,
provider tools, images, audio, embeddings, RAG, plus retries / failover /
caching), and the shared [`Cache`](docs/cache.md) / [`Storage`](docs/storage.md) /
[`Queue`](docs/queue.md) facades over the backend-agnostic
[`substrate-core`](docs/substrate.md) contract. See the
[changelog](CHANGELOG.md) and the [roadmap](docs/roadmap.md).

Every milestone is compiled, clippy-clean and tested — 895 Rust tests and 497
TypeScript and Svelte tests, with the IPC surface covered end to end and CI running across
macOS, Linux and Windows. GUI/OS integrations are launch-smoked, with visual /
side-effecting steps called out as unverified.

## Learn more

Full documentation lives at **[elyracode.com/framework](https://elyracode.com/framework)**.
Start with [getting started](docs/getting-started.md) and the
[architecture](docs/architecture.md) overview, then dive into any topic from the
[documentation index](docs/README.md).

## License

Licensed under the [MIT License](LICENSE).

See [CONTRIBUTING.md](CONTRIBUTING.md) and the
[Code of Conduct](CODE_OF_CONDUCT.md) to get involved.
