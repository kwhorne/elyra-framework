# Changelog

All notable changes to Elyra Framework are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While Elyra is pre-1.0, minor versions may contain breaking changes; these are
called out under **Changed** with a migration note.

## [Unreleased]

_Nothing yet._

## [0.5.2] — 2026-07-17

### Added

- **Command cancellation.** `invokeCancellable(command, ...args)` in
  `@elyra/runtime` returns `{ id, result, cancel }`; `cancel()` aborts the
  in-flight command on the Rust side (via a request-id header + a `/__cancel`
  route that aborts the command's task). Progress is done with the event bus
  (documented pattern) — no new API needed.
- **AI rate limiting + token budget.** `AiBuilder::rate_limit(per_minute)`
  throttles every provider call (waits, doesn't error); `token_budget(max)`
  refuses new prompts once cumulative tokens hit the cap (`Error::Budget`);
  `Ai::tokens_used()` reports the running total.
- **Opt-in CSP.** `App::csp(policy)` sets a `Content-Security-Policy` header on
  HTML responses served over `elyra://` (off by default — a too-strict policy
  can break the webview).

### Changed

- **Locks no longer poison.** Switched internal `std::sync::Mutex` to
  `parking_lot::Mutex` across the cache, event bus, sidecar, store, queue,
  windows, and the AI client — a panic while holding a lock can no longer
  cascade into a poisoned-lock crash.

### Fixed

- **Sidecar CPU spin.** If every command sender dropped while a child was still
  running, the owning task's `select!` busy-looped on a closed channel at 100%
  CPU. The command arm is now disabled once the channel closes; the task only
  waits on the child to exit.
- **Unbounded EventBus growth.** Emitted events buffered without limit when the
  frontend was gone/reloading/slow. The queue is now capped (`MAX_QUEUED`); when
  full the oldest half is dropped so a reconnecting frontend still gets recent state.
- **Cache TTL leak.** Expired entries were only reclaimed when the same key was
  read again. `CacheProvider` now starts a background sweeper (`Cache::sweep`)
  that drops expired entries periodically; it holds a `Weak` ref and stops when
  the cache is dropped.
- **Predictable updater temp files.** Downloaded updates were written to a static
  path in the temp dir. They now use an unpredictable filename, refuse to open a
  pre-existing path (`O_EXCL`), and are created `0600` on Unix — mitigating
  symlink attacks and collisions on shared machines.

### Changed

- **Migrations run in a transaction.** Each migration (and its history row) now
  commits atomically where the driver supports transactional DDL (SQLite,
  Postgres); a mid-file failure rolls back cleanly. MySQL auto-commits DDL, so
  partial state there remains possible — documented.

## [0.5.1] — 2026-07-17

### Added

- **`substrate-core` crate.** A tiny, dependency-free crate defining the shared,
  backend-agnostic `Cache` / `Storage` / `Queue` contracts behind the "one
  ecosystem" facades — the same traits the Askr/Laravel side can implement.
  Elyra's `Cache`, `Storage`, and `Queue` now implement them (re-exported as
  `elyra::substrate`); conformance is verified in `tests/substrate.rs`. `Cache`
  is byte-internal, so `substrate` `get`/`put` round-trip losslessly.

### Changed

- `Cache` stores values as bytes internally (the JSON/typed API is unchanged
  sugar on top). No behavior change for existing callers.

## [0.5.0] — 2026-07-17

### Added

- **Queue facade.** An in-process background job queue with the same surface as
  Laravel's `Queue::` — `push` a named job, register an async handler with `on`.
  Jobs run in order on a background task; status is emitted on `elyra:queue`
  (`onQueue`). Bind with `QueueProvider`; push from Rust (`ctx.get::<Queue>()`)
  or the frontend (`queue` in `@elyra/runtime`). In-process / non-durable by
  design (durable, cross-process queues are Askr's domain).
- **Storage facade.** A filesystem disk with the same surface as Laravel's
  `Storage::` — `put` / `get` / `append` / `exists` / `delete` / `size` /
  `files` / `url`, every path jailed to the disk root. Bind with
  `StorageProvider::at(root)`; use from Rust (`ctx.get::<Storage>()`) or the
  frontend (`storage` in `@elyra/runtime`).
- **Cache facade.** An ergonomic in-process, TTL-aware key-value cache with the
  same surface as Laravel's `Cache::` (and Askr's shared cache) — `get` / `put` /
  `add` / `remember` / `increment` / `forget` / `flush`, typed helpers, arbitrary
  JSON values. Bind with `CacheProvider`; use from Rust (`ctx.get::<Cache>()`) or
  the frontend (`cache` in `@elyra/runtime`). First of the shared "one ecosystem"
  facades that mirror the Askr/Laravel side over a local backend.

## [0.4.0] — 2026-07-15

### Added

- **AI reliability (`ai`).** Automatic **retries** with exponential backoff on
  transient failures / retryable statuses (`AiBuilder::retries` / `retry_backoff`),
  provider **failover** (`Chat::failover([...])`), and in-memory response
  **caching** for plain prompts (`AiBuilder::cache` / `cache_ttl`, `clear_cache`).
- **AI provider tools (`ai`).** Native, server-executed **web search** and
  **web fetch** (`web_search` / `web_fetch` on `Chat`, `WebSearch` / `WebFetch`
  / `UserLocation`). Anthropic-native; OpenAI returns `Unsupported` (Responses
  API not used yet).
- **AI audio (`ai`).** Text-to-speech (`ai.speech(...).generate()` →
  `GeneratedAudio`) and transcription (`ai.transcribe(bytes, name).generate()`),
  over OpenAI (`gpt-4o-mini-tts` / `whisper-1` defaults).

## [0.3.1] — 2026-07-14

### Added

- **AI SDK (`ai` feature).** A new Laravel-inspired `elyra-ai` crate, re-exported
  as `elyra::ai`: anonymous + named **agents** (`Agent`), **tools** with an
  automatic tool-use loop (`Tool`), **sub-agents** (an `Agent` used as a tool via
  `sub_agent` / `AgentTool`), **structured output** (`prompt_as::<T>` via
  `serde` + `schemars`), **streaming** (`stream` → `StreamChunk`, ideal for the
  event bus), **images**, **embeddings**, and an in-memory **vector store** for
  RAG (`VectorStore` + `cosine_similarity`) over Anthropic + OpenAI.
  `AiProvider` binds an env-configured `Ai` client into the container. Default
  text model `claude-sonnet-5`; images `gpt-image-1`.
- **Single-instance** (`App::single_instance`). Later launches focus the running
  window and forward their command line on `elyra:second-instance` (`onSecondInstance`),
  then exit. Portable loopback rendezvous with a per-app handshake.
- **Deep-linking** (`App::deep_link("myapp")`). Launch URL via `deepLink.initial()`,
  later URLs on `elyra:deep-link` (`onDeepLink`) — macOS open-URL event + Windows/Linux
  scheme registration; pairs with single-instance for while-running delivery.

## [0.3.0] — 2026-07-14

### Added

- **Sidecar processes (`sidecar` feature).** Spawn and manage child processes
  via `sidecar` in `@elyra/runtime` (`spawn` / `write` / `kill`) or the
  `elyra::sidecar::Sidecar` handle; `stdout`/`stderr` lines and exit stream on
  the `elyra:sidecar` channel (`onSidecar`). No extra crate — uses `tokio`.
- **Autostart (`autostart` feature).** Launch the app at login via `autostart`
  in `@elyra/runtime` (`enable` / `disable` / `isEnabled`) or the `elyra::autostart`
  module. Backed by `auto-launch` (LaunchAgents / registry / `.desktop`).
- **Settings store.** A persistent key-value store (`store` in `@elyra/runtime`,
  `Store` in the container) backed by `settings.json` in the OS config dir —
  `get` / `set` / `delete` / `all` / `clear`, arbitrary JSON values. Core, no
  feature flag.

- **Native application menu.** `App::menu(Menu::new().submenu(Submenu::new("File")…))`
  adds custom submenus (with accelerators) after the standard app + Edit menus;
  clicks emit `elyra:menu` (subscribe with `onMenu`). Rendered on macOS.
- **Global shortcuts (`shortcuts` feature).** `App::global_shortcut("CmdOrCtrl+Shift+P")`
  registers OS-level keyboard shortcuts; firing one emits the `elyra:shortcut`
  event (subscribe with `onShortcut`). Backed by `global-hotkey`.
- **Window-state persistence.** `App::persist_window_state()` remembers the
  primary window's size, position, and maximized state between runs (stored under
  the OS config directory, keyed by the About name). Dependency-free.
- **Window control + file drop.** `@elyra/runtime` exports `appWindow`
  (minimize / maximize / fullscreen / close / focus / show / hide / center /
  setTitle / setSize) with live state via `appWindow.onState`, and `onFileDrop`
  for native file drops. Backed by new `Windows` methods on the Rust side
  (usable from commands, with an optional target-window label). Core — no
  feature flag.
- **UI components in `@elyra/runtime`.** Themed, dependency-free primitives:
  `alert` / `confirm` / `prompt` dialogs, `toast()` notifications, a ⌘K
  **command palette** (`registerCommands` / `openCommandPalette`), and
  `contextMenu()`. They read the app's CSS variables, matching the About /
  update components.
- **System integration (`system` feature).** Native desktop essentials exposed
  through `@elyra/runtime`: file dialogs (`dialog.open` / `dialog.save`),
  opening URLs/files in the OS (`shell.openExternal`), the clipboard
  (`clipboard.readText` / `writeText`), OS notifications (`notify`), and
  standard paths (`paths`). Backed by `rfd`, `open`, `arboard`, `notify-rust`,
  and `dirs`; also usable from Rust via the `elyra::system` module.

## [0.2.0] — 2026-07-13

### Added

- **Models — relation auto-hydration.** Declare a relation on a *field*
  (`#[model(has_many(Book, fk = "author_id"))] books: Vec<Book>`) and the derive
  skips it as a column, defaults it to empty, and generates a `with_<field>`
  batch hydrator that fills it in one query — no more joining a `HashMap` by
  hand. Works for `has_many` (`Vec<T>`), `has_one` / `belongs_to` (`Option<T>`;
  `belongs_to` targets must be `Clone`).
- **Models — non-`i64` primary keys.** A single-column primary key may now be any
  type (e.g. `String`), marked with `#[model(id)]`. The value is app-supplied and
  included in the `INSERT` (no key read-back), and `find` takes that key type.
  The default `i64` autoincrement behaviour is unchanged. Composite keys remain
  unsupported.
- **Models — column-aware `belongs_to`.** The owning row is looked up against the
  related model's actual primary-key column (`<T>::PK`) instead of a hardcoded
  `id`, and the child's foreign key is read by column name — so `belongs_to`
  works even when the owner's PK column is renamed via `#[model(column = "..")]`.

- **Codegen:** serde container attributes are now reflected in the generated
  TypeScript via `specta-serde` — `rename` / `rename_all`, tagged and untagged
  enums (as discriminated unions), `flatten` (as intersections), and `skip`.
  Elyra's numeric policy (64-bit ints and floats render as `number`) is applied
  on top.
- **Database tests:** model CRUD now runs against real **MySQL** and
  **Postgres** servers in CI (`model_servers.rs`), exercising per-driver
  placeholders (`?` vs `$n`) and key retrieval (`last_insert_id` vs
  `RETURNING`). The tests are opt-in via `ELYRA_TEST_MYSQL_URL` /
  `ELYRA_TEST_POSTGRES_URL` and skip cleanly when unset.

### Changed

- **Updater:** `UpdaterConfig::auto_check` now defaults to `false`. The silent
  startup check (and its toast) is opt-in via `.auto_check(true)`, so apps no
  longer notify about updates on launch unless they ask to.
- **Dependencies:** updated to their latest releases — `sqlx` 0.9 (dynamic SQL
  is now wrapped in `AssertSqlSafe`), `ureq` 3, `ed25519-dalek` 3, `tray-icon`
  0.24 + `muda` 0.19, and a `cargo update` across the tree. No public API
  changes.
- **Tooling:** CI typechecks the runtime with **TypeScript 7** and runs on
  **Node 24**; `@msgpack/msgpack` bumped to `^3.1.3`.
- Docs: clarified that code signing, Apple ID / Developer ID, notarization, and
  binary distribution are the application's responsibility — not the framework's.
  Removed them from the roadmap and added an explicit "Out of scope" section.

## [0.1.0] — 2026-07-13

First public release. Everything below is compiled, `clippy`-clean, and tested
(SQLite for the database layer; GUI/OS integrations are launch-smoked, with
visual or side-effecting steps called out as unverified in the docs).

### Added

#### Core

- **`App` builder** — fluent assembly of a desktop app: window options,
  container bindings, providers, middleware, commands, and assets.
- **Container + `Ctx`** — a type-keyed service container resolvable from any
  command, provider, or background task (`ctx.get::<T>()`).
- **Providers** — two-phase `register` / `boot` wiring, like Laravel service
  providers.
- **Middleware pipeline** — outermost-first command middleware around dispatch.

#### IPC bridge

- **`elyra://localhost` custom protocol** — the whole app lives under one
  origin: assets, commands (`/__cmd/*`), and the event stream (`/__events`).
- **MessagePack wire format** — compact argument arrays in, named maps out
  (structs decode to JS objects); no JSON in the hot path.
- **`#[command] async fn`** — typed commands dispatched on a multi-thread tokio
  runtime; the UI thread never blocks. `Result` commands surface `Err` as a
  rejected promise (`CommandError`).

#### Events

- **`EventBus` + `channel()`** — Rust→frontend push over a multiplexed
  long-poll, batched per flush; a Svelte-readable store on the frontend.

#### Windows, tray, updater

- **Multi-window** — additional windows at startup or at runtime via the
  container-bound `Windows` handle.
- **System tray** (`tray` feature) — icon + menu; clicks arrive on the `tray`
  event channel.
- **Auto-updater** (`updater` feature) — ed25519-verified update manifest,
  semver comparison, HTTP fetch, and signature-checked staged download.
- **macOS application menu** — an Edit menu (so ⌘C/⌘V/⌘X reach the webview) and
  a custom About item.

#### Data (`database` feature)

- **`Database`** — one handle over SQLite / MySQL / Postgres via sqlx's `Any`
  driver, with per-driver placeholder rendering.
- **Migrations** — `rata migrate` with batches, reversible `down`, and status.
- **`#[derive(Model)]`** — Active Record with CRUD, a typed query builder
  (`where_*`, `where_in`, `order_by`, `limit`, `get`/`first`), `bool`↔INTEGER
  mapping, `#[model(column)]`, `#[model(timestamps)]`, relations
  (`has_many` / `has_one` / `belongs_to`), and N+1-avoiding eager loading
  (`load_<name>`).

#### Codegen & runtime

- **`rata codegen`** — specta types → TypeScript definitions and a typed
  `api.*` facade that mirrors every `#[command]`.
- **`@elyra/runtime`** — `invoke()`, `channel()`, and the generated `api.*`.

#### Tooling

- **Ratatosk (`rata`)** — `new` (scaffold with the Grove theme),
  `dev` (Vite HMR + `elyra://` IPC), `codegen`, `build`, `bundle`
  (macOS `.app` + ad-hoc signing), and `migrate`.

#### UI components

- **About dialog** — set metadata once with `App::about(AboutInfo::new(..))`;
  the shell serves it at `/__about` and `@elyra/runtime` renders a themed
  dialog. On macOS the standard **About &lt;App&gt;** menu item opens it; from
  the frontend, call `openAbout()`.
- **Update component** — `App::updater(UpdaterConfig::new(..))` adds a silent
  startup check, `/__update/check` + `/__update/install` endpoints, progress on
  the `elyra:update` channel, and a themed **update toast** in
  `@elyra/runtime` (available → install → download → restart).
  `Updater::apply_and_relaunch` replaces the running binary and re-execs.

[Unreleased]: https://github.com/kwhorne/elyra-framework/compare/v0.5.2...HEAD
[0.5.2]: https://github.com/kwhorne/elyra-framework/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/kwhorne/elyra-framework/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/kwhorne/elyra-framework/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/kwhorne/elyra-framework/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/kwhorne/elyra-framework/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/kwhorne/elyra-framework/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/kwhorne/elyra-framework/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kwhorne/elyra-framework/releases/tag/v0.1.0
