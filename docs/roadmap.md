# Roadmap

Milestones delivered so far, and what's next. Each shipped milestone is
compiled, clippy-clean, and tested (SQLite for the DB layer; GUI/OS integrations
are launch-smoked, with visual/side-effecting steps called out as unverified).

## Delivered

- **M0** — tao + wry + `elyra://` custom protocol + one `#[command]` end to end
  over MessagePack; container; assets from memory with a fallback page.
- **M1** — `EventBus` + `channel()` + per-flush batching; **async** custom
  protocol (no `block_on`). Both frontends include a latency probe.
- **M2** — specta codegen (`rata codegen`) → TS types + typed `api.*`; the CLI
  (`dev`, `codegen`, `build`).
- **M3** — providers (`register`/`boot`), the middleware pipeline, `Result`
  commands, window options.
- **M4** — multi-window + the `Windows` handle, `rata new` scaffolding,
  `rata bundle` (macOS `.app` + ad-hoc signing).
- **M5** — `elyra-db` (SQLite/MySQL/Postgres via sqlx `Any`) + `rata migrate`
  (batched, reversible, status).
- **M6** — system tray (`tray-icon`, clicks → event bus) and the auto-updater
  (manifest + semver + ed25519 verification; HTTP fetch + staged download).
- **M7** — `#[derive(Model)]` Active Record: CRUD + typed query builder,
  per-driver placeholders, injection-safe column identifiers.
- **M8** — model extras: `bool`↔INTEGER mapping, `#[model(column)]`, and
  `#[model(timestamps)]`.
- **M9** — model relations (`has_many` / `has_one` / `belongs_to`) and
  `where_in` for eager-load queries.
- **M10** — eager loading: each relation gets a `load_<name>` batch method
  (one `WHERE fk IN (..)` query, grouped into a `HashMap`) to avoid N+1.
- **UI components (v0.1.0)** — a built-in [About dialog](about.md)
  (`App::about`) and an [auto-update toast](updater.md) (`App::updater`), both
  rendered by `@elyra/runtime`; the updater now applies + relaunches
  (`Updater::apply_and_relaunch`).
- **Codegen serde support** — types export through `specta-serde`, so
  `rename` / `rename_all`, tagged / untagged enums, `flatten`, and `skip` are
  reflected in the generated TypeScript.
- **System integration (`system` feature)** — native file dialogs, `openExternal`,
  clipboard, OS notifications, and standard paths, exposed through
  `@elyra/runtime` (`dialog` / `shell` / `clipboard` / `notify` / `paths`).
- **Database CI** — model CRUD runs against real **MySQL** and **Postgres**
  service containers in CI (`framework/tests/model_servers.rs`), covering
  per-driver placeholders and key retrieval; skipped locally unless the
  `ELYRA_TEST_*_URL` env vars are set.
- **Desktop platform APIs (v0.3.0)** — the Electron-parity push:
  [UI components](components.md) (dialogs / toasts / ⌘K palette / context menu),
  [window control + file drop + state persistence](windows.md),
  [global shortcuts](shortcuts.md), a native [app menu](menu.md),
  a [settings store](store.md), [autostart](autostart.md),
  [sidecar processes](sidecar.md), and
  [single-instance + deep-linking](single-instance.md).
- **AI SDK (v0.3.1 → v0.4.0)** — a Laravel-inspired [`elyra::ai`](ai.md): agents,
  tools, sub-agents, provider tools (web search / fetch), structured output,
  streaming, images, audio (TTS / STT), embeddings, an in-memory vector store for
  RAG, plus retries / provider failover / response caching, over Anthropic +
  OpenAI (`ai` feature).
- **Shared facades (v0.5.0)** — [`Cache`](cache.md), [`Storage`](storage.md), and
  [`Queue`](queue.md) with the same ergonomics as the Askr/Laravel side, over
  local backends.
- **Shared substrate contract** — the [`substrate-core`](substrate.md) crate
  formalizes the backend-agnostic `Cache` / `Storage` / `Queue` traits; Elyra's
  facades implement them (verified in `tests/substrate.rs`), ready for Askr to
  depend on the same crate.

- **Hardening + Laravel parity (v0.5.7)** — an IPC token + capability model
  and origin-scoped CORS, a default CSP, deny-by-default sidecar spawn and
  policy-gated `shell.open`, bounded/depth-checked request bodies, an authenticated
  single-instance handshake, HTTPS + streaming in the updater, and OS-keychain
  [`Secrets`](secrets.md). Plus [`Log`](logging.md), [`Config`](configuration.md),
  [`TestApp`](testing.md), the [schema builder](migrations.md) with Rust
  migrations + seeders, Eloquent-style pagination/aggregates/joins/soft
  deletes/transactions, queue retries + DLQ, typed event channels in codegen,
  asset `ETag`/`Range` caching, a cross-platform app menu, `.deb`/portable
  bundling, and CI across macOS/Linux/Windows. See the
  [changelog](../CHANGELOG.md).

- **Security + structure (v0.5.8)** — [per-command abilities](security.md#4-per-command-abilities)
  (`#[command(can = "…")]` + `App::allow_ability`), so `Capability::Commands` is
  no longer one grant over every registered command; [`Secrets`](secrets.md)
  returns a value that zeroizes on drop; `ForbiddenError` names the gate that
  refused instead of always blaming the token; `shell.rs` split into a `shell/`
  module with access control isolated in `guard`; plus `elyra::prelude` and
  `rata make:middleware`.

- **Laravel depth (v0.6.0)** — models: [`belongs_to_many`](models.md#many-to-many-belongs_to_many),
  [casts](models.md#casts), local + global [scopes](models.md#scopes) and
  [factories](models.md#factories); container: trait-object and lazy bindings;
  [domain events](events.md#domain-events-dispatcher) with `App::broadcast` to a
  typed frontend channel; [durable queues](queue.md#durable-queues);
  [test fakes](testing.md#fakes) with `App::swap`.

- **Desktop depth (v0.7.0)** — [per-command middleware](middleware.md#per-command-middleware)
  with aliases and groups; [clock-time scheduling](scheduler.md#clock-times) and
  cron, sleep- and DST-aware; [validation](validation.md) for nested input,
  arrays and the database (44 rules); [translations](i18n.md) shared by Rust and
  Svelte.

- **Resources (v0.8.0)** — [`rata make:resource`](resources.md): the commands,
  validation, events and tests for a model, `--view` for the Svelte screens,
  `--generate` for the model, migration, factory and seeder from a field list; a
  [hash router](frontend-runtime.md#routing) and a registry behind them;
  translated validation messages and [typed `$t`](i18n.md#typed-keys).

- **Live queries (v0.9.0)** — [`#[command(live)]`](live-queries.md): reads
  recorded, writes reported, the affected subscriptions re-run (coalesced,
  unchanged results skipped) and pushed to the windows watching; typed
  `live.*` stores on the frontend; live lists and detail views from
  `make:resource`.
- **AI agents (v0.10.0)** — [every app an MCP server](mcp.md): granted
  commands as tools with JSON Schemas from their types, `myapp --mcp` into the
  running app or headless, confirmation in the client, live commands as
  resources with subscriptions, an event and a channel for every call, rate
  limits, and `rata mcp install`.
- **MCP resource templates (v0.11.0)** — [live commands with arguments](mcp.md#resource-templates)
  as URIs an agent fills in and watches (`app://customers_show/{id}`),
  with completion for enums and booleans.
- **A Laravel backend (v0.12.0)** — [the desktop half of a Laravel app](backend.md):
  `Http`, `Backend` with Laravel's answers mapped (a `422` per field), Sanctum
  sign-in with the token in the keychain, live queries over the API, and
  `make:resource --backend`; CI against a real Laravel app.
- **Laravel's broadcasting in (v0.13.0)** — [changes made on the web](backend.md#broadcasting-changes-made-on-the-web)
  update the open windows: the Pusher protocol (Askr, Reverb, Pusher), live
  queries re-run, `BackendEvent`, `onBackendEvent`, and `make:resource --backend`
  following its channel; CI through Reverb and Askr.
- **Working offline (v0.14.0)** — [the last answer to read, an outbox to write](backend.md#working-offline):
  a per-user copy of every `GET`, `.queue_offline()` writes sent once with an
  `Idempotency-Key`, conflicts with keep mine / take theirs, the `connection`
  and `outbox` stores, and `make:resource --backend` offline; CI pulls the plug
  on a real Laravel app.
- **Laravel's notifications (v0.15.0)** — [`$user->notify(..)` on the desktop](backend.md#laravels-notifications):
  in the OS when the app isn't in front, a toast when it is, the unread count
  on the dock, the list read and marked everywhere (offline through the
  outbox), what was missed caught up, and `rata make:notifications`; CI
  through Reverb.
- **Coding agents (v0.16.0)** — [the framework for the agent working on the app](ai-agents.md):
  `rata ai:install` (guidelines in `AGENTS.md`, five skills, `.mcp.json`),
  `rata mcp dev` (app info, schema, migrations, the log, the guides), and
  `elyracode/elyra-backend` with Laravel Boost guidelines for the Laravel half.
- **Files (v0.17.0)** — [uploads and downloads with the Laravel backend](backend.md#files):
  multipart uploads streamed from disk, downloads streamed to it and marked,
  `ChosenFile` grants from dialogs and drops, `422`/`413` on the field,
  uploads queued offline with a copy, the `transfers` store, and a `file`
  field in `make:resource --backend`; CI against a real Laravel app.
- **Abilities (v0.18.0)** — [what the signed-in user may do, by Laravel](backend.md#what-the-user-may-do):
  `Backend::abilities()` and `Route::elyraAbilities()` (spatie or the Gate),
  `can` commands refused locally (offline too), kept current by focus,
  reconnect and `AbilitiesChanged`, `$can(..)`, and generated screens that
  hide what the user can't do; CI with spatie through Reverb.
- **The current Claude API (v0.19.0)** — the [AI SDK](ai.md)'s Anthropic driver
  on API version `2026-10-21`: no `temperature`, structured outputs instead of
  a forced tool call, paused turns resumed, `Error::Refused`, and Claude
  Sonnet 5.5 as the default model.
- **The app's assistant (v0.20.0)** — [an assistant in the app](assistant.md)
  ([RFC 0011](proposals/0011-in-app-assistant.md)): the app's commands as its
  tools through the MCP catalog and one call path, held to the signed-in
  user's abilities, confirmed in the app; Claude through
  `Route::elyraAssistant()` with the user's Sanctum token (or the user's own
  key, or `AiProvider`); the panel, the `assistant` store and the ⌘K entry;
  `rata assistant inspect`, the bundle's key check, `Ai::fake` and bearer
  tokens in the AI SDK; CI against a real Laravel app.
- **`@elyra/ui` (v0.21.0)** — [desktop components](ui.md)
  ([RFC 0012](proposals/0012-elyra-ui.md)): 31 Svelte 5 components on one
  set of design tokens, which the runtime's built-ins read too; `can`,
  Laravel's validation per field, the runtime's confirm and `live.*` tables
  built in; keyboard-first with ARIA; `make:resource --view` on them, `rata
  make:ui` to own one, a skill for coding agents, and a reference with every
  example running.
- **The assistant sees the window (v0.22.0)** —
  [RFC 0013](proposals/0013-assistant-sees-the-window.md): each turn carries
  the window's route, its live queries (only those the user's assistant may
  call) and what the app says is selected, as names and ids, never rows;
  `#[command(link = "..")]` turns a step in the panel into a link to its
  record, and `make:resource` writes it. `@elyra/ui` gains the
  `OutboxDrawer`, opened from the StatusBar.
- **More desktop controls (v0.23.0)** — from RFC 0012's "later" list:
  [`Combobox`](ui/combobox.md) (async options, a resource's `_index` as the
  source, debounced, the stale answer aborted),
  [`FileField`](ui/file-field.md) (the native dialog and a dropped file;
  `make:resource`'s file field now) and [`SplitPane`](ui/split-pane.md)
  (a keyboard-movable separator, its size kept in the settings store).
- **The Elyra Inspector (v0.24.0)** —
  [RFC 0014](proposals/0014-elyra-inspector.md): a developer's drawer in
  debug builds, compiled out of a release. An ability simulator that only
  takes away, through one mask the guard, `can`, the assistant and MCP all
  read; the window note as the assistant sees it; the window's live
  queries and calls. Also row selection in `<Table>` (told to the
  assistant), a `Combobox` for references in `make:resource`, and `Kbd`.
- **Offline, dates and ⌘K search (v0.25.0)** — the inspector's
  [Simulate offline](inspector.md#trying-the-app-offline) unplugs the
  backend for the run, so the offline copy, the outbox and broadcasting
  can be tried for real; [`DatePicker`](ui/date-picker.md) and
  [`Avatar`](ui/avatar.md) in `@elyra/ui`; and the ⌘K palette
  [searches the app's records](components.md#searching-the-apps-records),
  which `make:resource --view` registers for each resource.
- **Presence (v0.26.0)** — [RFC 0015](proposals/0015-presence.md): who else
  has a record open, on the web or the desktop, from Laravel's presence
  channels, joined once for all windows; [`<Presence>`](ui/presence.md) in
  `@elyra/ui` and `make:resource --presence`.
- **Presence in the inspector (v0.27.0)** — the drawer's Presence
  section: the joined channels, their members and windows; and presence
  tested against Reverb in the Laravel CI job.
- **Capabilities per window (v0.28.0)** — [RFC 0016](proposals/0016-window-capabilities.md):
  a window [given less](windows.md#giving-a-window-less) than the app grants,
  known by its own protocol handler; client ids bound to their window.

## Next / open

- **Desktop and web, with a Laravel backend** — the web half is Laravel (on
  [Askr](https://github.com/kwhorne/askr)). After [RFC 0005](proposals/0005-laravel-backend.md)
  (v0.12.0: sign-in, the API, its errors) and [RFC 0006](proposals/0006-laravel-broadcasting.md)
  (v0.13.0: Laravel's broadcasting in, so a change on the web updates the open
  windows) and [RFC 0007](proposals/0007-offline-and-sync.md) (v0.14.0:
  working offline, with an outbox that syncs back). Then
  [RFC 0008](proposals/0008-laravel-notifications.md) (v0.15.0: Laravel's
  notifications on the desktop) and [RFC 0009](proposals/0009-files.md)
  (v0.17.0: files, up and down) and [RFC 0010](proposals/0010-laravel-abilities.md)
  (v0.18.0: what each user may do, by Laravel). Next:
  [RFC 0011](proposals/0011-in-app-assistant.md) (v0.20.0: an assistant in the
  app, over its own commands and the user's abilities). Next:
  [RFC 0012](proposals/0012-elyra-ui.md) (v0.21.0: `@elyra/ui`, desktop
  components for Svelte that know the app). Next:
  [RFC 0013](proposals/0013-assistant-sees-the-window.md) (v0.22.0: the
  assistant sees the window it's asked from, and its steps link to the
  records they touched). Next:
  [RFC 0014](proposals/0014-elyra-inspector.md) (v0.24.0: a developer's
  drawer in debug builds, with an ability simulator that only takes away,
  the window as the assistant sees it, the live queries and the calls).
  Next: [RFC 0015](proposals/0015-presence.md) (v0.26.0: presence, who else has
  a record open, on the web or the desktop, from Laravel's presence
  channels).

- **Codegen** — optional `bigint` transport for integers beyond 2^53
  (MessagePack already carries `i64` on the wire; this is a TS-typing opt-in).
- **Models** — composite (multi-column) primary keys. (Single non-`i64` keys,
  relation **auto-hydration** into struct fields, and **column-aware relation
  FKs** have shipped.)
- **AI SDK** — additional providers (Gemini, Groq, …) and OpenAI provider tools
  via the Responses API; live provider integration tests in CI.
- **Live queries** — finer invalidation than per table (rows, or the query's
  own `WHERE`) if measurements ask for it; database change capture (SQLite
  `update_hook`, Postgres `LISTEN/NOTIFY`) to also catch raw SQL and other
  processes. Both deliberately out of v1 ([RFC 0002](proposals/0002-live-queries.md#decisions)).
- **MCP** — completions an app computes for a template's variables and a
  prompt's arguments (customer names, say), beyond the enums and booleans
  the schema gives. (Prompts have shipped, shared with the assistant's
  suggestions.)
- **Dogfood** — port a real app to pressure-test the DX.
- **Installers** — MSI (WiX) / NSIS on Windows and AppImage/Flatpak on Linux;
  `rata bundle` currently produces a `.deb`, a portable `.tar.gz` and a portable
  Windows folder.
- **Capabilities per window** — shipped in v0.28.0
  ([RFC 0016](proposals/0016-window-capabilities.md)). Next, if apps ask for
  it: a `.read_only()` shorthand, and events scoped per window.

## Out of scope

Elyra is a framework, not a release pipeline. It deliberately does **not** handle
code signing, Apple ID / Developer ID, notarization, or building and shipping
binaries — those belong to each application's own release process. `rata bundle`
produces a local, ad-hoc-signed `.app` for development; producing a signed,
notarized, distributable build is the app's responsibility.

The updater is the same: it provides verified download + apply primitives, but
generating keys, signing artifacts, and hosting the manifest are the app's job,
not the framework's.

## Known sharp edges

- Events use HTTP-style long-poll over the custom protocol (wry's async
  responder is one-shot). One connection is multiplexed across channels.
- macOS requires the webview on the main thread; the tokio runtime is kept
  separate and every request is spawned onto it.
- The `#[command]` macro handles neither generics nor `Option<Ctx>` — deliberate.
- `rata dev` relies on cross-scheme `fetch` (http origin → `elyra://`); CORS is
  set, but platform behavior there is unverified.
