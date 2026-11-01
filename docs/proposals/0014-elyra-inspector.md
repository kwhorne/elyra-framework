# RFC 0014 — Elyra Inspector

**Status:** accepted (2026-10-31) · **Target:** 0.24.0 · builds on [RFC 0002](0002-live-queries.md), [RFC 0010](0010-laravel-abilities.md), [RFC 0011](0011-in-app-assistant.md) and [RFC 0013](0013-assistant-sees-the-window.md) · see [Decisions](#decisions)

## Summary

A drawer for the developer, in debug builds only. It shows what the
framework knows about the window and lets the developer take abilities
away to see what a user without them gets:

```text
⌘⌥E (Ctrl+Alt+E)                                  Elyra Inspector · debug build
┌ Abilities ─────────────────────────────────────────────────────────────────┐
│ customers.view     ● on                                                     │
│ customers.delete   ○ off (simulated)   ← the Delete button, the command     │
│ invoices.*         ● on                  and the assistant's tool all go    │
├ Window ────────────────────────────────────────────────────────────────────┤
│ <window>{"route":"/customers/42","showing":[{"command":"customers_show",…  │
├ Live queries ──────────────────────────────────────────────────────────────┤
│ customers_show {id: 42}   reads customers   pushed 3×, last 2 s ago         │
├ Calls ─────────────────────────────────────────────────────────────────────┤
│ 14:02:11  customers_update   ok      12 ms   212 B → 1.4 KB                 │
│ 14:02:09  customers_destroy  denied   0 ms   (customers.delete)             │
└────────────────────────────────────────────────────────────────────────────┘
```

- **Ability simulator.** Each ability the app knows has a switch. Turning
  one off takes it away everywhere at once: the `can` store, the shell's
  guard, the assistant's tools and the window note. It can only take
  away, never give.
- **The window as the assistant sees it.** The exact `<window>` note the
  next turn would carry (RFC 0013), and the system prompt's rules.
- **Live queries.** The window's subscriptions: command, arguments, the
  tables they read, how often they've been pushed.
- **Calls.** The last commands the window called: name, outcome, time and
  sizes. Not their arguments or results.

## Motivation

Abilities decide a lot in an Elyra app, and they decide it in several
places:

- `can="customers.delete"` hides the button (RFC 0012).
- The shell's guard refuses the command (RFC 0010).
- The assistant isn't offered the tool, and its note leaves out the live
  queries the user may not run (RFC 0011, RFC 0013).
- An MCP agent's grant is filtered the same way (RFC 0003).

To check all of that today, the developer changes the user's role in
Laravel, signs in again, and looks. Then changes it back. It's slow, so it
doesn't get done, and the place that was missed is the one that ships.

The rest of the framework's state is just as hidden. What the assistant
was told about the window, which live queries a screen holds open, and
which call was refused and why, are all in Rust. The WebView's own
devtools see a `fetch` to `elyra://localhost/__cmd/x`, with MessagePack
bodies they can't read.

## Current state (0.23)

- **Abilities** come from two places. `App::allow_ability` is the app's
  grant to the frontend (`Policy::grants_ability`). With a backend, the
  signed-in user's come from Laravel (`Abilities::allows`, RFC 0010). The
  guard needs both. The assistant's `offered()` and the window note
  filter by the same caller. The runtime's `can` store reads
  `/__auth/abilities`.
- **The window note** is built by `Assistants::note` from the route, the
  window's frontend subscriptions (`LiveRegistry::showing`) and its
  selection.
- **Live subscriptions** live in `LiveRegistry`, with the tables each read
  and the last value pushed.
- **Calls** go through the shell's router. Nothing records them after
  they're answered.
- **Dev mode** is `ELYRA_DEV_URL`, which `rata dev` sets. It decides CORS
  for the dev server. It is an environment variable, so anyone can set it
  on a release binary. It isn't a security boundary.

## Design

### Debug builds only

All of the inspector is behind `#[cfg(debug_assertions)]`: the routes, the
call log, the mask, and the runtime's drawer is only opened when the shell
says it has an inspector. A release build (`rata build`, `cargo build
--release`) contains none of it. There's no flag to turn it on in a
release, and setting `ELYRA_DEV_URL` doesn't.

An app can also turn it off in debug: `App::inspector(false)`.

### Opening it

- **⌘⌥E** on macOS, **Ctrl+Alt+E** elsewhere, in any window.
- `openInspector()` from `@elyra/runtime`.
- `@elyra/ui`'s `StatusBar` shows an "Inspector" item in a debug build.

It's a drawer in plain DOM, drawn by `@elyra/runtime` like the assistant
panel, so it works in an app without `@elyra/ui`, or without Svelte.

### Ability simulator

```text
GET  /__inspector/abilities   → [{ ability, granted, user, masked }]
POST /__inspector/mask        { ability, off: bool }
```

- **The list** is every ability the app knows: those its commands declare
  (`#[command(can = …)]`), those it grants, and the user's from Laravel.
  Each says whether the app grants it, whether the user has it, and
  whether it's masked.
- **The mask** is a set of abilities turned off for this run. It's checked
  in `Policy::grants_ability` and `Abilities::allows`, the two places every
  check already goes through. So the guard, the `can` store, the
  assistant's tools, the window note and an MCP agent all see it, with no
  second path to keep in step.
- **Off only.** Masking an ability takes it away. Unmasking gives back what
  was there. Nothing the app and Laravel didn't grant can be turned on.
- **A namespace** (`customers.*`) can be masked whole.
- **Visible while it's on.** The drawer's header, and the StatusBar item,
  say "Simulating: −customers.delete" while anything is masked. The mask
  lasts until the app quits.
- **The runtime follows.** A change is sent on `elyra:abilities`, as a
  refresh from Laravel is, so `can` and every `can="…"` update at once.
- **Refused calls say so:** "you may not do this: it takes the
  `customers.delete` ability (simulated by the inspector)".

### The window as the assistant sees it

```text
GET /__inspector/window   → { note, rules, tools }
```

- `note` is the `<window>` block the next turn from this window would
  carry: the same `Assistants::note`, with the same filtering.
- `rules` is the system prompt's fixed part (with no user name).
- `tools` is the list the user's assistant is offered now.
- Without an assistant, the section says so.

### Live queries

```text
GET /__inspector/live     → [{ command, arguments, reads, pushes, last_push, opened }]
```

The window's subscriptions from `LiveRegistry`, newest first. Arguments
are decoded to JSON. `reads` are the tables the last run read, which are
what a write must touch to re-run it.

### Calls

```text
GET /__inspector/calls    → [{ at, command, outcome, ms, sent, received, ability? }]
```

- A ring of the last 200 calls per window: commands, live opens, and the
  assistant's turns.
- **`outcome`** is `ok`, or the error's kind (`denied`, `forbidden`,
  `validation`, `not-found`, …). A refusal names its ability.
- **Sizes, not contents.** Arguments and results aren't kept. They can hold
  anything the user typed. The live section shows the arguments of what's
  on screen.
- The drawer follows it on `elyra:inspector` while it's open.

## Security

- **Compiled out of release builds.** It isn't hidden behind a setting
  that could be flipped. The routes don't exist in a release binary.
- **The same gate as everything else.** In debug, the routes need the IPC
  token and only answer the app's own windows.
- **The simulator only takes away.** A mask can't grant an ability, so a
  bug in it, or a script calling `/__inspector/mask`, can only make the
  app refuse more.
- **No contents in the call log.** The log is names, outcomes, timings and
  sizes.

## Alternatives considered

- **A separate devtools window or app** (like React DevTools). More room,
  but another process and another connection to secure. A drawer reuses
  the IPC that's already there.
- **An `@elyra/ui` component.** Tidier in a Svelte app, but an app on
  another framework, or none, would have no inspector.
- **Granting abilities in the simulator too.** Useful for trying a screen
  as an admin, but it turns a dev tool into a way past the guard. The
  developer can grant in Laravel.
- **Logging arguments.** More useful for debugging, and a place where
  personal data piles up. The live section shows what's on screen.

## Implementation plan

1. **The mask** in `security.rs` and `abilities.rs`, with the
   `/__inspector/abilities` and `/mask` routes, behind
   `cfg(debug_assertions)`. Tests that a masked ability is refused by the
   guard, missing from `can`, not offered to the assistant, left out of
   the note, and refused to MCP. A release-profile test that the routes
   don't exist.
2. **The drawer** in `@elyra/runtime`, with the hotkey and
   `openInspector()`, and the abilities section.
3. **Window, live queries and calls:** the three routes, the call ring in
   the router, and their sections.
4. **The StatusBar item** in `@elyra/ui`, and the docs.

## As built

Step 1:

- **The mask is an `elyra::inspector::Mask`**, bound in the container and
  held by the `Policy`. In a release build it's empty and has no way to be
  set, and `shell::inspector` (the routes) isn't compiled.
- **It's checked in four places, not two.** The guard refuses a masked
  command before its grant (`denied`, "… (simulated by the inspector)"),
  so the developer sees why. `Caller::denied` checks it for every AI
  caller, so the assistant's tools, its window note and an MCP agent's
  calls lose it. `Abilities::allows` checks it for Rust code asking about
  the user. `AbilitiesState` carries `masked`, which the `can` store
  reads.
- **An MCP agent's `tools/list` keeps the tool.** The list is fixed for
  the process and cached by the client for an hour. A call is refused.
- **The routes are POSTs**, as every IPC route is: `/__inspector/abilities`
  and `/__inspector/mask` `{ ability, off }`. A name that isn't an
  ability or a namespace is refused. A change is sent on `elyra:abilities`
  and re-runs the live queries that asked.
- **`App::inspector(false)`** turns the routes off in a debug build.
- **A release build is tested**: `tests/inspector_release.rs`, run by CI
  with `cargo test --release`.

Steps 2 to 4:

- **The drawer polls** each section once a second while it's open,
  instead of following `elyra:inspector`. The inspector's own requests
  aren't in the call log, so polling doesn't fill it. Abilities are
  redrawn only when they change, so a switch isn't replaced under the
  pointer.
- **The drawer opens on the left**, beside the assistant's panel on the
  right, so both can be open.
- **The key is matched by its code** (`KeyE`): Option turns E into ´ on a
  Mac.
- **The window section** is `{ note, rules, tools }`, or `{ assistant:
  false }` in an app without one. `rules` is the system prompt without
  the date and the user's name.
- **A live row** has `command`, `arguments` (as sent), `reads`, `pushes`
  and `pushed_ago_ms`. Only the window's own subscriptions, not an
  agent's.
- **A call row** has `at`, `command` (`live:x` for a live query opened,
  `assistant` for a turn asked), `outcome`, `ms`, `sent`, `received`, and
  `ability` when it was refused for one. A guard's refusal is logged too.
- **`@elyra/ui`'s StatusBar** reads `inspector` off the runtime module,
  so an older runtime has no button rather than a build error.
- **The smoke test** opens the drawer with Ctrl+Alt+E in Chrome, turns
  `customers.delete` off, checks `can`, gives it back and closes it.

After 0.24, the offline switch:

- **`/__inspector/offline` `{ on? }`** unplugs the backend for the run.
  The flag (`inspector::Unplugged`) is shared by every clone of the
  `Backend`, and checked where a request goes out to the network, where
  the health route is probed, where broadcasting starts, and where the
  assistant reaches Claude through Laravel. Each answers as a network with
  no server: `HttpError::Unreachable`, "offline, simulated by the
  inspector". The offline copy, the outbox and the probe then do what
  they'd do for real.
- **Turning it on** takes the connection offline at once, with
  `simulated: true` on `elyra:connection`, and closes broadcasting.
  **Turning it off** asks the health route at once, and opens broadcasting
  again only if it was open, or a sign-in asked for it meanwhile.
- **The drawer's Network section** is shown when the app has a backend.
  The banner and the StatusBar's button name both: "Simulating: offline
  −customers.delete", and "Stop simulating" undoes both.

## Open questions

1. **Where it's drawn:** in `@elyra/runtime`, plain DOM, for every app
   (recommended), or as an `@elyra/ui` component?
2. **The hotkey:** ⌘⌥E / Ctrl+Alt+E (recommended; ⌘⌥I is the usual key
   for a browser's devtools, which an app may enable in its WebView), or
   something else?
3. **The mask and MCP:** does an MCP agent see the mask too (recommended:
   it's one set of abilities for the app), or only the window?
4. **Arguments in the call log:** never (recommended), or behind a
   checkbox in the drawer?
5. **How long the mask lasts:** until the app quits (recommended), or kept
   across restarts in debug?

## Decisions

Settled on 2026-10-31, all as recommended:

1. **It's drawn by `@elyra/runtime`**, in plain DOM, so every app has it.
2. **⌘⌥E on macOS, Ctrl+Alt+E elsewhere.**
3. **An MCP agent sees the mask too.** It's one set of abilities for the
   app.
4. **The call log never keeps arguments.** Names, outcomes, timings and
   sizes only.
5. **The mask lasts until the app quits.** It isn't kept across restarts.
