# Changelog

All notable changes to Elyra Framework are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While Elyra is pre-1.0, minor versions may contain breaking changes; these are
called out under **Changed** with a migration note.

## [Unreleased]

### Added

- **Codegen:** serde container attributes are now reflected in the generated
  TypeScript via `specta-serde` — `rename` / `rename_all`, tagged and untagged
  enums (as discriminated unions), `flatten` (as intersections), and `skip`.
  Elyra's numeric policy (64-bit ints and floats render as `number`) is applied
  on top.

### Changed

- **Updater:** `UpdaterConfig::auto_check` now defaults to `false`. The silent
  startup check (and its toast) is opt-in via `.auto_check(true)`, so apps no
  longer notify about updates on launch unless they ask to.
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

[Unreleased]: https://github.com/kwhorne/elyra-framework/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/kwhorne/elyra-framework/releases/tag/v0.1.0
