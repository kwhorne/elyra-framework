# Elyra Framework documentation

A Rust + Svelte 5 framework for hyper-responsive desktop apps — Laravel's
ergonomics (container, providers, a typed bridge), compiled and binary, with no
runtime overhead. The CLI is **Ratatosk** (`rata`).

> Also hosted at **[elyracode.com/framework](https://elyracode.com/framework)**.

## Start here

- [Getting started](getting-started.md) — install, scaffold, run.
- [Architecture](architecture.md) — the big picture and the Laravel map.
- [Configuration (`elyra.toml`)](configuration.md) — the project descriptor.

## Backend (Rust)

- [Commands](commands.md) — `#[command]`, `Ctx`, `Result` commands.
- [Container & providers](container-and-providers.md) — DI + wiring.
- [Middleware](middleware.md) — the dispatch pipeline.
- [Events](events.md) — `EventBus`, batched Rust→frontend push.
- [Windows](windows.md) — window config, multi-window, control, persistence, file drop.
- [Application menu](menu.md) — native app menu from `App::menu` (macOS).
- [About dialog](about.md) — built-in About window from `App::about`.
- [System integration](system.md) — dialogs, shell-open, clipboard, notifications (`system` feature).
- [Global shortcuts](shortcuts.md) — OS-level keyboard shortcuts (`shortcuts` feature).
- [System tray](tray.md) — tray icon + menu (`tray` feature).
- [Auto-updater](updater.md) — ed25519-verified updates (`updater` feature).

## Data (Rust)

- [Database](database.md) — SQLite / MySQL / Postgres via one `Database`.
- [Migrations](migrations.md) — `rata migrate`, batches, rollback.
- [Models](models.md) — `#[derive(Model)]` Active Record + query builder + relations.

## Frontend & bridge

- [Frontend runtime](frontend-runtime.md) — `@elyra/runtime`: `invoke`, `channel`, `api.*`.
- [UI components](components.md) — dialogs, toasts, ⌘K command palette, context menu.
- [Codegen](codegen.md) — specta → TypeScript types + typed `api.*`.
- [Wire format](wire-format.md) — the binary IPC contract.

## Tooling

- [Ratatosk CLI](cli.md) — `new`, `dev`, `codegen`, `build`, `bundle`, `migrate`.
- [Roadmap](roadmap.md) — milestones and what's deferred.
- [Changelog](../CHANGELOG.md) — released versions and changes.

## Crate layout

```
framework/   elyra          — App, Container/Ctx, Command, events, shell (tao+wry)
macros/      elyra-macros   — #[command] and #[derive(Model)]
database/    elyra-db       — Database, migrations, models (GUI-free)
ratatosk/    ratatosk       — the `rata` CLI
runtime/     @elyra/runtime — invoke(), channel(), generated api.*
example/     elyra-example  — the demo app / DX benchmark
```

## Cargo features (on the `elyra` crate)

| Feature | Enables |
|---|---|
| `database` | `Database`, `Model`, migrations, `App::database` |
| `tray` | `App::tray`, the system tray |
| `updater` | the `updater` module (ed25519) |

```toml
elyra = { version = "0.1", features = ["database", "tray", "updater"] }
```
