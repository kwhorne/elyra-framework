# RFC 0025 — A command's docs in the bindings, a command called from Rust, and the configuration checked at once

**Status:** implemented (2026-11-06) · **Target:** 0.37.0 · builds on [RFC 0011](0011-in-app-assistant.md) and [RFC 0016](0016-window-capabilities.md) · see [Decisions](#decisions)

## Summary

Three small things about how an app is put together, each a gap between
what Elyra already knows and what the developer sees:

```rust
/// Archive a customer: hidden from the lists, kept for the invoices.
#[command(can = "customers.archive")]
async fn customers_archive(ctx: Ctx, id: i64) -> Result<Customer> { … }
```

```ts
// bindings.ts: the doc comment and the ability, in the editor's hover
export const api = {
  /**
   * Archive a customer: hidden from the lists, kept for the invoices.
   *
   * @remarks Needs `customers.archive`.
   */
  customers_archive(id: number): Promise<Customer> { … },
};
```

```rust
// From Rust: the command is still a function, with its own types
let customer = customers_archive::run(ctx.clone(), 7).await?;
```

```text
the app's configuration has 2 problems:
  - App::stay_in_tray() needs a tray: add .tray(TrayConfig::new()..)
  - Backend::listen(..) needs broadcasting to listen to the channels:
    add .broadcasting(Broadcasting::pusher(key)) to the Backend
```

## Motivation

- **The frontend calls commands it can't read.** A command's doc comment
  is its MCP tool's description and the assistant's (RFC 0003, 0011), but
  `bindings.ts` has only the name and the types: the Svelte side hovers
  `api.customers_archive` and learns nothing of what it does or which
  ability it needs. Both are in the registry when the file is written.
- **A command stops being a function.** `#[command]` turns the function
  into a unit struct and hides its body behind MessagePack. Another
  command that wants its logic, or a Rust test that wants to call it with
  its own types, can't: it goes through `TestApp::invoke("name", (..))`,
  with the name as a string and the arguments as an untyped tuple, or the
  logic is moved into a second function by hand.
- **A wrong configuration is found one problem at a time.** `prepare`
  stops at the first `assert!` it meets; fix it, run again, and meet the
  next. And not all of them stop the app: a broken updater configuration
  is a log line and an update flow that silently isn't there.

These come from comparing Elyra with [Rune](https://github.com/rune-rs/rune)'s
Rust embedding: `#[rune::function]` keeps the function callable and its
docs in the tooling, and building a unit collects every diagnostic before
reporting. The rest of Rune (the VM itself, a code mode for the assistant)
is a separate question, for its own RFC if at all.

## Current state (0.36)

- **Codegen** (`framework/src/codegen.rs`): `api.*` is written from
  `CommandSig { name, args, ret }`. `Command::description()`, `ability()`
  and `live()` exist (`command.rs`); the description is used by MCP and the
  assistant, not by codegen. Types already carry their own docs into
  `bindings.ts` through specta.
- **`#[command]`** (`macros/src/lib.rs`): emits `struct name;` and an
  `impl Command`, with the original body as `__elyra_inner` inside
  `call`. The doc comment becomes `description()`; other attributes on the
  function (an `#[allow(..)]`, say) are dropped with it.
- **Checks at start** (`App::prepare`, `app.rs`): `assert!`s for
  `stay_in_tray()` without a tray, and for `Backend::notifications()`,
  `presence(..)` and `listen(..)` without broadcasting; an `expect` for
  `UpdaterConfig::from_backend(..)` without a backend; a `panic!` for
  middleware names that don't resolve and for an assistant that doesn't
  attach. An `UpdaterConfig` that `build()` refuses (no key, a
  `developer_id` that doesn't check) is logged, and the update flow is
  off.
- **Docs**: `docs/validation.md`'s first example has no `Ctx` parameter,
  which the macro requires; it wouldn't compile as written.

## Design

### 1. A command's docs in `bindings.ts`

`rata codegen` writes a JSDoc block above each `api.*` function and each
live store, from what the registry already has:

- the **description**, as written (Markdown is fine in JSDoc hovers);
- `@remarks`, when there's something to say: `Needs \`customers.archive\`.`
  for a `can = "…"`, and `Live: subscribe with \`live.customers_list()\`.`
  for a `#[command(live)]`.

A command without a doc comment, ability or live gets no block, so a
small app's `bindings.ts` doesn't change. `*/` in a doc comment is written
as `*\/`. Codegen reads `description()`, `ability()` and `live()` from the
command, so `CommandSig` (public, and built by hand in a hand-written
`Command`) doesn't change.

### 2. A command called from Rust

`#[command]` also emits the function itself, as an associated function on
the struct:

```rust
impl customers_archive {
    /// Archive a customer: hidden from the lists, kept for the invoices.
    pub async fn run(ctx: Ctx, id: i64) -> Result<Customer> { … }
}
```

`run` has the function's own signature, visibility, `async`-ness, doc
comment and attributes; `Command::call` decodes the arguments and calls
it. So `.commands(commands![customers_archive])` works as before, and
`customers_archive::run(ctx, 7).await` is a checked call with real types.

`run` is the function, not the pipeline: no middleware, no ability check,
no window scope, no rate limit. That's what dispatching from Rust already
is ("Rust-side dispatch is never gated", `Command::ability`): those are
limits on what the webview may reach, and Rust is the app itself. A test
that wants the pipeline keeps `TestApp::invoke`. Validation inside the
body runs either way.

The struct and its function share the command's name: no new name to
learn, and nothing to import beyond what's imported today.

### 3. The configuration checked at once

The checks in `prepare` become one pass that collects every problem
before anything starts (before providers boot, before a window or a
socket opens), and reports them together:

| Problem | Today |
|---|---|
| `stay_in_tray()` without a tray | `assert!` |
| `notifications()` / `presence(..)` / `listen(..)` without broadcasting | `assert!` each |
| `UpdaterConfig::from_backend(..)` without a backend | `expect` |
| An `UpdaterConfig` that `build()` refuses | logged, updates off |
| A middleware name that doesn't resolve | `panic!` |

Each problem says which call it's about and how to fix it, worded as the
message is today. `App::problems(&self) -> Vec<Problem>` gives the list
without starting anything, for a test:

```rust
#[test]
fn the_app_is_configured() {
    assert_eq!(app::build().problems(), vec![]);
}
```

`run()` returns `Err(Error::Config(..))` with the list instead of
panicking, so `fn main() -> elyra::Result<()>` prints it and exits
non-zero; `prepare()`, which returns no `Result`, panics with it. The
assistant's check stays after the providers boot, as it is: a provider may
bind the assistant, and it's checked against the commands then.

### What it doesn't do

- **No `elyra::Module`.** Grouping commands, models and prompts under one
  name is mostly `commands![..]` and a `Provider` already; scoping a
  window by module is `only_abilities(["timers.*"])` with a `can = "…"`
  on each command. Worth it once a large app shows the need.
- **No warnings.** A problem is something that would fail or do nothing;
  advice isn't a configuration check.
- **No source spans.** The configuration is a builder chain, not source
  Elyra parses; the call's name finds it.

### Security

- `run` adds no reach: it's Rust calling Rust, and the webview reaches a
  command only through the pipeline, as before.
- `@remarks` in `bindings.ts` names an ability the frontend already sees
  in an `ability` error and in `can()`; nothing new leaves the app.
- An updater configuration that can't verify an update now stops the
  start instead of running without updates: the app was meant to update,
  and a log line was easy to miss.

### Testing

- Codegen: a command with a doc, an ability and live gets the block; one
  with none gets none; `*/` is escaped; a live store gets it too.
- The macro: `run` is callable with its types and returns what `call`
  encodes; a sync command's `run` is sync; doc and attributes reach it
  (a `#[allow(..)]` on a command no longer warns in clippy); `compile_fail`
  doctests for what still doesn't compile.
- Checks: each problem alone, and all of them together in one list;
  `problems()` empty for a valid app; `run()` returns the error; the
  existing `should_panic` tests keep their messages.
- Mutation tests over the codegen block, `run`'s delegation and the checks.

## Implementation plan

1. Codegen: the JSDoc block, from the registry. Docs: `codegen.md`.
2. The macro: `run`, `call` through it, the attributes kept. Docs:
   `commands.md` (calling from Rust, and what it skips), `testing.md`.
3. `Problem`, `App::problems()`, `prepare`/`run` reporting all of them;
   the updater's refusal as a problem. Docs: `configuration.md`.
4. `docs/validation.md`'s example with its `Ctx`.
5. CHANGELOG with **Upgrading** (a broken updater configuration now stops
   the start), roadmap.

## As built

- **`elyra::Error` had no `Config`.** The design named `Error::Config(..)`;
  it was the updater's error type that had one. `elyra::Error` has it now,
  carrying `Problems` (the list, `Problems::list()`). The enum isn't
  `#[non_exhaustive]`, so an exhaustive `match` needs the arm: in the
  CHANGELOG's **Upgrading**.
- **`Debug` for `Error` is written by hand.** `fn main() -> elyra::Result<()>`
  prints the error with `{:?}`; derived, it was `Config(...)` around the
  list. `Config` now prints the list alone; every other variant prints as
  derived (tested).
- **What a provider binds is checked after boot, as before:** the database
  for `Backend::offline()` (an `expect`) and the assistant. Both can come
  from a provider's `register`, so `problems()` can't see them; the design
  named the assistant only.
- **The middleware check lists every command wired wrong,** one message
  each, by command name. `finalize`'s `panic!` stays behind it, unreachable
  in practice, as do the `expect` for `from_backend(..)` and the updater's
  log line.
- **`run` carries the function's attributes** and its doc comment, plus a
  line saying it isn't the pipeline; the struct carries the doc comment too,
  for rustdoc. A `#[cfg(..)]` on the function now applies to `run` only (it
  was dropped with the function before).
- **The JSDoc's live remark** reads ``Live: `live.name(..)` re-runs it when
  the data it read changes.``, `(..)` only when the command takes arguments.
- **Checked:** 1121 Rust tests passed before the last changes; new tests in
  `tests/codegen.rs`, `tests/command_run.rs` (including that the webview is
  refused a `can = "…"` command `run` reaches, and a clippy-checked
  `#[allow(clippy::too_many_arguments)]`) and `tests/problems.rs`; mutation
  testing over the three parts (see the PR).

## Alternatives considered

- **A typed call through the pipeline** (`app.call(customers_archive, (7,))`):
  middleware and abilities from Rust. Rust code doesn't need the webview's
  limits, and `TestApp::invoke` covers testing the pipeline; it can come
  later on top of `run`.
- **Doc fields on `CommandSig`**: breaks a hand-written `Command` that
  builds one, for data the registry already has.
- **Keeping `prepare`'s `assert!`s and adding `problems()` beside them**:
  two lists of the same checks that drift.
- **Rune's `Diagnostics`, with warnings and spans**: the configuration
  isn't parsed source; a list of problems is the part that helps.

## Open questions

1. **The name of the direct call:** `name::run(ctx, ..)` (recommended:
   short, and reads as running the function), `name::invoke(..)` (but
   `TestApp::invoke` goes through the pipeline, and this doesn't), or
   `name::call(..)` (collides with `Command::call`)?
2. **What `@remarks` says:** the ability and live (recommended: what a
   caller needs to know before calling), or the link and middleware too?
3. **`run()` with problems:** returns `Err` with the list (recommended:
   `main` prints it and exits non-zero, no panic backtrace), or panics as
   `prepare()` does?
4. **An updater configuration `build()` refuses:** a problem that stops
   the start (recommended: the app meant to update, and verification is
   the point), or a log line, as today?
5. **`App::problems()` public** (recommended: a one-line test that the
   app is configured), or only reported at start?

## Decisions

Settled on 2026-11-06, all as recommended:

1. **The direct call is `name::run(ctx, ..)`.**
2. **`@remarks` says the ability and live.**
3. **`run()` returns `Err` with the list;** `prepare()` panics with it.
4. **An updater configuration `build()` refuses is a problem** that stops
   the start.
5. **`App::problems()` is public.**
