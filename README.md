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
  [providers](docs/container-and-providers.md); every configuration problem
  reported at once, before anything starts ([`App::problems()`](docs/configuration.md#the-apps-configuration-checked-at-start)).
- **`EventBus` + `channel()`** — Rust→frontend push, batched per flush over a
  long-poll; the UI thread never blocks. ([events](docs/events.md))
- **Typed bridge** — `rata codegen` turns specta types into TS + a typed
  `api.*` facade, with each command's doc comment and ability as its JSDoc;
  in Rust a command is still a function, `name::run`. ([codegen](docs/codegen.md))
- **Windows, tray, updater** — [multi-window](docs/windows.md),
  [system tray](docs/tray.md), and an [ed25519-verified updater](docs/updater.md).
- **Desktop components** — [`@elyra/ui`](docs/ui.md): Svelte 5 buttons, menus,
  forms, selects, tables, dialogs, tabs and the window's shell, on one set of
  design tokens (motion that stops for a user who asked for less, and more
  contrast for one who asked for more), keyboard-first, and aware of the app: a `Table` over a live
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
  so no key ships in the app. In [code mode](docs/assistant.md#code-mode)
  Claude writes a loop that calls the commands from Anthropic's sandbox.
- **Agents that know the framework** — [`rata ai:install`](docs/ai-agents.md)
  gives coding agents the framework's guidelines and skills, and `rata mcp dev`
  answers with the app's commands, models, schema, log and the guides, and
  [sees the running debug build](docs/ai-agents.md#the-running-app): its
  windows as accessibility snapshots, their pictures, clicks and typing —
  Laravel Boost for the Rust side, with a Composer package for the Laravel half.
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
elyra = { version = "0.41.0", features = ["database", "tray", "updater", "system", "shortcuts", "secrets"] }
```

## Status

**v0.41.0** — **each account's changes, counted**: `Account.pending`
and `.failed` for every server a user is signed in to, active or not, and
`<AccountSwitcher>` shows them under each server. In 0.40.1,
**the running app, checked on macOS too**: CI walks a
coding agent through the example app's window on macOS, built and under
`rata dev`, as on Linux and Windows. In 0.40.0, **the bell, the account
menu and the tree** in `@elyra/ui`:
`<NotificationBell>` for Laravel's notifications, `<AccountSwitcher>` for
the servers a user is signed in to, and a keyboard-first `<TreeView>` with
children read as branches open. In 0.39.1,
**the desktop's settings and updates from Laravel**:
`Route::elyraBroadcasting()` and `Route::elyraUpdates()` in
`elyracode/elyra-backend`, with `php artisan elyra:release` to put a
release up, checked in CI against the real Laravel app. In 0.39.0,
**the tray's icon, and the app's**: `TrayConfig::icon(..)`
with `.template()` for the macOS menu bar, `ctx.get::<Tray>()` to change
its title, tooltip and icon while the app runs, the unread count on the
tray, and `App::icon(..)` for the windows on Windows and Linux
([RFC 0028](docs/proposals/0028-tray-and-icons.md)); a remembered window
opens on a screen that's there. In 0.38.2, **`rata dev` on every
platform**: it runs npm on Windows, closing the app stops Vite, and CI
walks through the app under `rata dev` on Linux and Windows. In 0.38.1,
**Windows works**: the page reaches the
app at the origin WebView2 serves it from (`http://elyra.localhost`), and
CI drives the example app's window on Windows as on Linux. In 0.38.0:
**the assistant's code mode, and Linux that works**:
`Assistant::code_mode()` lets Claude call the app's tools from code in
Anthropic's sandbox, one loop instead of a turn per record, each call
checked as before and confirmations still the user's
([RFC 0027](docs/proposals/0027-assistant-code-mode.md)); on Linux a window
opens and the page's requests carry their bodies (WebKitGTK 2.40+), checked
in CI by driving the example app's window under Xvfb. See
[code mode](docs/assistant.md#code-mode). In 0.37:
**commands, configuration, and the running app for coding
agents**: a command's doc comment and ability reach `bindings.ts`, a command
is still a function Rust can call (`name::run`), and `App::problems()`
reports every configuration problem at once
([RFC 0025](docs/proposals/0025-commands-and-configuration.md)); `rata mcp
dev` reads a debug build's windows as accessibility snapshots, takes their
pictures, and clicks and types in them
([RFC 0026](docs/proposals/0026-the-running-app-for-coding-agents.md)); and
`@elyra/ui` moves a little, not at all for less motion, with more contrast
when asked. See [coding agents](docs/ai-agents.md#the-running-app). In 0.36:
**a window listens, and an app updates from its own
server**: `Backend::listen([..])` with the runtime's `listen(name, handler)`
lets a window follow a private channel while it shows what the channel is
about ([RFC 0023](docs/proposals/0023-window-channels.md)), and
`UpdaterConfig::from_backend(..).developer_id(..)` updates an app from the
server it's signed in to, verifying a whole macOS bundle by its Developer ID
signature ([RFC 0024](docs/proposals/0024-backend-updates.md)). See
[the updater](docs/updater.md). In 0.35:
**signed in to more than one server**:
`Backend::chosen_at_sign_in().multiple_servers()` keeps the user signed in to
each server they sign in to, one active, and `Auth::switch_to(server)` moves
between them without signing out; each server has its own token, offline
copy and outbox ([RFC 0022](docs/proposals/0022-multiple-servers.md)). See
[the backend](docs/backend.md#signed-in-to-more-than-one-server). In 0.34:
**a server chosen at sign-in**: `Backend::chosen_at_sign_in()`
lets the user type their company's server on the sign-in screen, sign in
through the browser with `Auth::sign_in_with_token(..)`, and switch between
workspaces without signing out
([RFC 0021](docs/proposals/0021-backend-chosen-at-sign-in.md)). In 0.33:
**an app that stays in the tray**: `App::stay_in_tray()` starts
with the tray icon only, runs on when its window closes, and stays out of the
Dock until a window shows ([RFC 0020](docs/proposals/0020-stay-in-tray.md)). In 0.32:
**a window that opens from the tray**: `TrayConfig::popover(..)`
opens a small window of the app's under the tray icon on a click, and hides
it on a click elsewhere, scoped as any window
([RFC 0019](docs/proposals/0019-tray-popover.md)). In 0.31:
**the assistant's answer, as it's written**: the panel shows
what Claude writes as it writes it, through the turn's tool steps, over the
user's key and through Laravel's `Route::elyraAssistant()` alike; Stop keeps
what was written ([RFC 0018](docs/proposals/0018-assistant-streaming.md)).
See [the assistant](docs/assistant.md). In 0.30:
**record invalidation and completions**: over the Laravel
backend, a write or a broadcast naming customer 40 re-runs the lists and
what showed customer 40, not every view of customers: one request to the
server instead of eleven ([RFC 0017](docs/proposals/0017-record-invalidation.md)).
A prompt's argument completes from the app's own data, over MCP and in the
assistant's panel. See [the backend guide](docs/backend.md#live-queries). In 0.29:
**who's editing, prompts and column widths**: `<Presence
editing>` shows "Ada is editing" both ways between the web and the desktop,
as Echo whispers it; an app's prompts are listed to an MCP agent and
suggested in the assistant's panel; and `<Table resizable persist>` keeps the
user's column widths. See [presence](docs/ui/presence.md#whos-editing). In 0.28:
**capabilities per window**: a window can be given less than
the app grants, `WindowConfig::new("preview").only_abilities(["invoices.view"])
.deny(Capability::Assistant)`, enforced at the shell's gate, in the assistant
and in `can`. See [windows](docs/windows.md#giving-a-window-less). In 0.27:
**presence in the inspector**: the drawer shows the presence
channels the windows joined and who's in each, and RFC 0015 runs end to end
in CI against a real Laravel channel rule and Reverb. See
[the inspector](docs/inspector.md). In 0.26:
**presence: who else has a record open**, on the web or the
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

Every milestone is compiled, clippy-clean and tested — 1173 Rust tests and 620
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
