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

## Next / open

- **Codegen** — optional `bigint` transport for integers beyond 2^53
  (MessagePack already carries `i64` on the wire; this is a TS-typing opt-in).
- **Models** — composite (multi-column) primary keys. (Single non-`i64` keys,
  relation **auto-hydration** into struct fields, and **column-aware relation
  FKs** have shipped.)
- **Dogfood** — port a real app to pressure-test the DX.

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
