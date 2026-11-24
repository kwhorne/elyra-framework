# RFC 0054 — The `gui` feature: Elyra without a window

**Status:** accepted (2026-11-24) · **Target:** 0.55.0 · builds on [container & providers](../container-and-providers.md), [commands](../commands.md), [MCP](../mcp.md), [RFC 0053](0053-exit-hooks.md) · see [Decisions](#decisions)

## Summary

`tao` and `wry` (and, on Linux, GTK and WebKitGTK) are hard dependencies of
the `elyra` crate. A command-line tool that shares an app's code (its
commands, its database layer, its updater) can't be built without them, so
it can't be built on a server, in a minimal container, or anywhere GTK
isn't installed. ElyraSQL's `elyrasql-cli` is that tool. Its fork of the
framework has a `gui` feature for exactly this.

This RFC puts the window behind a `gui` feature, on by default:

```toml
# The app, as today: `gui` is a default feature.
elyra = { …, features = ["database", "updater"] }

# Its command-line twin: no tao, no wry, no GTK.
elyra = { …, default-features = false, features = ["database", "updater"] }
```

```rust
// The CLI: the same commands and services, no event loop.
let app = App::new().provider(Db::provider()).command(export_csv);
let prepared = app.prepare();                       // container, registry, bus
let ctx = prepared.ctx;
let rows = export_csv(ctx.clone(), query).await?;   // or call it by name
```

- **Without `gui`**, what an app is made of stays: `App`'s configuration,
  the container and `Ctx`, commands and their registry, the event bus,
  codegen, migrations and seeders, `--mcp` over stdio, the test harness,
  and the features that need no window (database, http, backend,
  secrets, ai, updater, sidecar, autostart, microphone).
- **What needs a window goes with it**: the event loop, windows, the tray,
  the menu, the popover, Quick Look, the matrix and Point at it, global
  shortcuts, and the file dialogs.
- **Nothing changes for an app**: `gui` is a default feature, and `tray`,
  `shortcuts` and `system` imply it.

## Motivation

- **ElyraSQL** ships `elyrasql-client` (the window) and `elyrasql-cli`
  (scripts, CI, servers) from one crate. The CLI must build where GTK
  doesn't exist. Moving off its fork needs this upstream.
- **Any app with a CLI twin, a server-side worker or a headless test job**
  sharing its commands gets the same: `cargo build` without system webview
  packages.
- **It's mostly there already.** `--mcp`, `ELYRA_CODEGEN_OUT`,
  `ELYRA_MIGRATE`, `ELYRA_SEED` and `ELYRA_CHECK` all run without opening
  a window. `TestShell` drives the IPC router without a webview. Only the
  crate's dependencies say otherwise.

## Current state (0.53)

- `framework/Cargo.toml`: `tao` and `wry` are unconditional. On Linux, so
  are `muda` (with `gtk`), `webkit2gtk`, `gio` and `cairo-rs`; on macOS the
  `objc2-*` crates for the popover and snapshots; on Windows
  `webview2-com`. `default = []`.
- Thirteen source files name `tao` or `wry`. Most are in `shell/`, plus
  `window.rs`, `dev.rs`, `diagnostics.rs` and `app.rs`.
- The IPC layer (`shell/router.rs`, `guard.rs`, `protocol.rs`) uses only
  `wry::http`, which is the `http` crate re-exported. `shell/mod.rs` keys
  windows by `tao::window::WindowId`.
- `Prepared` carries `windows`, `tray`, `menu`, `icon` and `shortcuts`.
- `system` pulls in `rfd` (file dialogs, GTK on Linux) along with
  `open`, `arboard`, `notify-rust` and `dirs`.

## Design

### Features

```toml
[features]
default = ["gui"]
# The window: tao, wry and the system webview (WebKitGTK on Linux), the
# app menu, and the platform crates the shell uses.
gui = ["dep:tao", "dep:wry", "dep:muda", "dep:webkit2gtk", "dep:gio", "dep:cairo-rs",
       "dep:objc2", "dep:objc2-app-kit", "dep:objc2-foundation", "dep:objc2-web-kit",
       "dep:block2", "dep:webview2-com"]
tray = ["gui", "dep:tray-icon"]
shortcuts = ["gui", "dep:global-hotkey"]
system = ["gui", "dep:rfd", "dep:open", "dep:arboard", "dep:notify-rust", "dep:dirs"]
```

The platform crates become optional under their `[target.…]` tables. The
`http` crate becomes a direct dependency (it already is one, through wry),
so the IPC layer names it, not `wry::http`.

### What moves behind `gui`

- `shell/webview.rs`, `shell/picture.rs`, `shell/dev.rs`,
  `shell/diagnose.rs` (the event loop, the windows, the snapshots).
- `window.rs` (`Windows`, `WindowConfig`'s runtime half), `tray.rs`,
  `menu.rs`, `popover.rs`, `matrix.rs`, `point.rs`, the shortcuts, the
  Quick Look parts, the badge and the window icon.
- `App`'s builder methods for them (`window`, `size`, `tray`, `menu`,
  `popover`, `stay_in_tray`, `shortcut`, `matrix`, `icon`, …), and
  `Prepared`'s matching fields.
- The routes that act on a window (`/__window/*`, Quick Look, the popover)
  answer "this build has no window" without `gui`, rather than not
  compiling, so the router stays one piece.

The window's *description* (`WindowConfig`'s label, path, scope and
abilities) stays, because a window's scope is part of the IPC security
model that `TestShell` exercises.

### What stays without `gui`

The router, the guard, the protocol, `TestShell`, the client ids
(`Clients` keyed by an opaque id instead of tao's `WindowId`), the event
bus, live queries, the inspector's data, codegen, `--mcp`, `rata check`'s
`ELYRA_CHECK`, migrations and seeders, and the exit hooks of RFC 0053.

### `App::run` without `gui`

It still runs every headless mode (`ELYRA_CODEGEN_OUT`, `ELYRA_MIGRATE`,
`ELYRA_SEED`, `ELYRA_CHECK`, `ELYRA_BUNDLE_INSPECT`, `--mcp`). Otherwise
it returns `Error::Config("this build has no window: elyra's `gui` feature
is off")` instead of opening one. A CLI calls `App::prepare()` and uses
its `Ctx`, or `--mcp`.

### Calling commands without a page

`Prepared` gets `call(name, args) -> Result<Value>`, the same dispatch
(middleware, validation, abilities against the app's own grants) the
router uses, for a CLI that wants its commands by name. A CLI can also call
the Rust functions directly. `call` is for a tool that takes a command name
from its own arguments (open question 3).

### Testing

- **CI**: a Linux job on a runner **without** the webview packages builds
  and tests `elyra` with `--no-default-features --features
  database,http,backend,secrets,ai,updater,sidecar`. If anything still
  pulls in GTK, the build fails at link time.
- `cargo hack --each-feature`-style checks for the features that imply
  `gui`, so each combination compiles.
- A small `examples/cli.rs`: prepare, call a command, print. It's built
  and run in that job.
- The existing suites run unchanged with the defaults.

## Implementation plan

1. `http` as a direct dependency; the IPC layer stops naming `wry::http`.
   `Clients` keyed by an opaque id. This is a no-op refactor that lands
   first.
2. The features and optional dependencies. `#[cfg(feature = "gui")]` on
   the modules, `App` methods and `Prepared` fields above, and the
   no-window answers for window routes.
3. `App::run` without `gui`; `Prepared::call`.
4. The CI job, the example, docs (`getting-started`, a "Without a window"
   section in `architecture.md`, the feature table in the README and
   `configuration.md`), CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **`gui` as a default feature, or explicit?**
   *Recommendation:* default. Every app today has a window, and `cargo add`
   should keep giving one. A CLI opts out with `default-features = false`,
   which is the usual Rust way to ask for less.
2. **`system` without dialogs**: split it so a CLI keeps `open`,
   clipboard, notifications and `dirs` without `rfd` (GTK on Linux)?
   *Recommendation:* not now: `system` implies `gui`. Notifications and the
   clipboard need a desktop session anyway. A CLI that wants `dirs` uses
   the crate. If a headless use for part of `system` turns up, it gets its
   own feature then.
3. **`Prepared::call` by name**: worth it, or should a CLI call the Rust
   functions?
   *Recommendation:* add it. It's a thin wrapper over the dispatch the
   router has, and it gives a CLI the app's middleware, validation and
   abilities. Calling the function directly skips all three, which is
   right for some tools and wrong for others.
4. **The crate's docs on docs.rs-style builds**: document every feature
   together (`--all-features`), as `cargo doc` in CI already does.
   *Recommendation:* yes, with `#[cfg_attr(docsrs, doc(cfg(feature = "gui")))]`
   on what needs it, so the docs say what needs a window.

## Not in this RFC

- **A headless browser** (rendering pages without a window).
- **`elyra-macros` and `elyra-db` changes**: neither depends on the
  window.
- **`rata` itself**, which is a separate binary and already has no
  webview.

## Alternatives considered

- **Splitting the crate** (`elyra-core` + `elyra`): a cleaner line, but
  every app's imports and every doc page would move, for what a feature
  gives without either.
- **The CLI depends on `elyra-db` alone**: works for the database, but not
  for the commands, the container, the updater or MCP, which is what
  ElyraSQL shares.

## Decisions

1. **`gui` is a default feature**; a CLI opts out with
   `default-features = false`.
2. **`system` implies `gui`** and isn't split now.
3. **`Prepared::call(name, args)`**, through the router's dispatch
   (middleware, validation, abilities).
4. **The docs say what needs a window**, with `doc(cfg(feature = "gui"))`.
