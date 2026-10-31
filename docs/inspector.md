# The inspector

A drawer for the developer, in debug builds only
([RFC 0014](proposals/0014-elyra-inspector.md)). Press **⌘⌥E** on macOS or
**Ctrl+Alt+E** elsewhere, or click **Inspector** in `@elyra/ui`'s
[StatusBar](ui/status-bar.md). It shows four things:

- **Abilities.** Every ability the app knows: those its commands declare,
  those it grants, and the signed-in user's from Laravel. Each one has a
  switch that takes it away.
- **The window, as the assistant sees it.** The `<window>` note its next
  turn would carry, the tools it's offered, and the rules it's given.
- **Live queries.** The window's subscriptions: the command and its
  arguments, the tables it reads, and how often it's been pushed.
- **Calls.** The window's last 200 calls: the command, the outcome (`ok`,
  `denied` and the ability it needed, `validation`, …), the time and the
  sizes.

## Trying a screen without an ability

Turn `customers.delete` off, and everything that checks it says no at
once:

- **`can`** says no, so a `Button can="customers.delete"` hides.
- **The shell** refuses the command: `denied`, "you may not do this: it
  takes the `customers.delete` ability (simulated by the inspector)".
- **The assistant** isn't offered the tool, and its window note leaves
  out the live queries that need it.
- **An MCP agent's** call is refused. Its `tools/list` keeps the tool,
  since the client caches the list.
- **`Abilities::allows`** says no in Rust.

A namespace (`customers.*`) can be turned off whole. The drawer, and the
StatusBar's button, say "Simulating" while anything is off. The mask
lasts until the app quits.

It only takes away. An ability the app doesn't grant, or the user doesn't
have, can't be turned on here. Grant it in the app or in Laravel.

## Not in a release

All of it is behind `#[cfg(debug_assertions)]`. A release build (`rata
build`, `cargo build --release`) has no inspector routes, the mask masks
nothing, and the key does nothing. Setting `ELYRA_DEV_URL` doesn't change
that. CI runs `tests/inspector_release.rs` in release to check it.

In a debug build it's on. `App::inspector(false)` turns it off:

```rust
App::new().inspector(false)
```

## From code

`@elyra/runtime` has the drawer and the routes under it:

```js
import { inspector, openInspector, closeInspector } from "@elyra/runtime";

await inspector.available();                 // false in a release
await inspector.mask("customers.delete", true);
const calls = await inspector.calls();       // newest first
```

A test does it in Rust: `app.get::<elyra::inspector::Mask>().set("customers.delete", true)`.

## What it keeps

The call log has names, outcomes, timings and sizes, never arguments or
results, which can hold anything the user typed. The live section shows
the arguments of the queries on screen. Nothing is kept across restarts.
