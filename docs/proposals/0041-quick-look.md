# RFC 0041 — Quick Look: a record at a glance, from the list, without leaving it

**Status:** implemented (2026-11-20) · **Target:** 0.50.0 · builds on [RFC 0012](0012-elyra-ui.md), [RFC 0016](0016-window-capabilities.md) and [RFC 0019](0019-tray-popover.md) · see [Decisions](#decisions)

## Summary

In Finder, Space on a file shows it in a window that follows the
selection: ↓ shows the next one, Space again closes it, and the list never
lost its place. A business app's list wants the same: look at a customer,
an invoice, a work order, without opening it, losing the scroll, the
search and the filters, and finding the way back.

```svelte
<Table
  page={customers}
  bind:query
  columns={["name", "email"]}
  onrowclick={(c) => goto(`/customers/${c.id}`)}
  preview={(c) => `/customers/${c.id}`}       <!-- Space: Quick Look -->
/>
```

- **Space on a row** opens a Quick Look window beside the app's window,
  showing the app's own route for that row (usually its detail view).
  Focus stays in the list.
- **↓ and ↑ move it along**: the preview shows the row the list is on, at
  once, as the user walks the list. Space or Esc closes it.
- **It's read-only by the shell**, not by the page: a Quick Look window
  can run the app's live queries and nothing that writes, so its Edit and
  Delete buttons hide and a script in it can't change anything.
- **`WindowConfig::read_only()`** is the same rule for any window, the
  shorthand RFC 0016 left for later.

## Motivation

- **Opening a record to look at it costs the list.** The route changes,
  the list's state goes with it (or has to be kept by hand), and coming
  back is a click and a scroll. Most looks don't need an edit.
- **A desktop app can have a second window.** The web would open a modal
  over the list, hiding it; a window beside it keeps both in sight.
- **It's the safe kind of window by nature.** A preview is for reading,
  and often of content someone else wrote (an invoice's text, a note's
  Markdown). RFC 0016 made narrowing a window possible; this makes the
  narrowest useful one, read-only, one line.

## Current state (0.49)

- **`<Table>`**: ↓/↑ move between rows that open, Enter and Space open a
  row (`onrowclick`), and with `select`, Space selects. Nothing previews.
- **Windows** (RFC 0016) can be narrowed: `only_abilities`,
  `deny_abilities`, `deny(Capability::X)`. There's no "only reads".
- **A live command** (`#[command(live)]`) can't write *while it runs as a
  live query*: a model-layer write, or a write to the backend, fails with
  "`x` is a live command and must only read". The same command called once
  through `api.*` is an ordinary call, and could write.
- **The popover** (RFC 0019) is a window made at start, hidden, shown and
  hidden at once, its page kept: the pattern Quick Look needs.

## Design

### The window

One Quick Look window for the app, label `quick-look`, made the first time
it's asked for and then hidden, not closed, so the next look is at once.

- **Beside the window it came from**: to its right when the screen has
  room, else its left, else over it; as tall as that window, a third of its
  width (at least 360 px), or the size the user last gave it.
- **It doesn't take focus.** The list keeps the keyboard; a click in the
  preview focuses it, and Esc there closes it.
- **Above its window**, not above every app: it belongs to that window, and
  hides with it, and when the app goes to the background.
- **One at a time**: a second list's Space moves it.

### Read-only

`WindowConfig::read_only()`, which the Quick Look window has:

- **Only live commands run, and only as reads.** A command that isn't
  `#[command(live)]` is refused in that window (`denied`, "this window only
  reads"), whatever its ability, from the page and from the assistant
  there. A live one is run under the same guard as a live query's re-run,
  whether it's subscribed to or called once, so a model-layer write or a
  write to the backend in it fails. Raw SQL and a command's own I/O are
  outside that guard, as they are for live queries.
- **`can` follows.** An ability no live command needs is "no" in that
  window, so `can="customers.delete"` hides the Delete button; one a live
  command needs (`customers.view`) is as it was.
- **The capabilities that change something** are denied: the store's
  writes, the clipboard's write, `shell.openExternal`, dialogs, the
  sidecar, the microphone, the updater, diagnostics' save. Reading the
  store, events and live queries stay.
- **It only narrows**, as every window limit does: it gives the window
  nothing the app hasn't, and composes with `only_abilities`.

### In the page

```ts
import { quickLook } from "@elyra/runtime";

quickLook.show("/customers/42");    // open it, or move it there
quickLook.hide();
$quickLook;                         // { open, route }
quickLook.here();                   // true in the Quick Look window itself
```

`<Table preview={fn}>` is that, wired to the keyboard: Space toggles it
for the row the list is on, and while it's open every ↓/↑ shows the new
row. A detail view can use `quickLook.here()` to leave out what makes no
sense in a narrow, read-only window (its breadcrumb, its Edit button,
though `can` hides that already).

`make:resource --view` passes `preview` to the generated list, pointing
at the record's own route, so a new resource has it.

### The route

The preview shows the app's own page at the route, as any window does
(RFC 0016's `path`): the hash router's `#/customers/42`. Moving it is a
route change in that window, not a reload, so its live queries change
with it, and what it showed is gone from memory once it moves on.

### Security

- **Read-only is the shell's**: a refused write never reaches the command,
  and the page in the window can't lift it (the shell knows the window by
  its own protocol handler, not by what the page says).
- **The page in the main window** can only point the preview at one of the
  app's own routes: `quickLook.show` takes a route, never a URL.
- **The preview's assistant**, if the app has one, is offered only live
  tools there.

### Testing

- **Read-only**: a non-live command refused from the window, a live one
  run, a live one that writes refused at the write (model layer and
  backend), `can` per ability, the denied capabilities, combined with
  `only_abilities`.
- **The window**: made once, hidden and shown, placed beside its window
  on one and two screens, not focused, hidden with its window.
- **`<Table preview>`**: Space opens, ↓ moves, Esc and Space close; with
  `select` (below), the other key.
- **By hand** on macOS, Windows and Linux: focus stays in the list, and
  the preview follows without flicker.

## Implementation plan

1. `WindowConfig::read_only()` in the shell: the command check, `can`, the
   capabilities.
2. The Quick Look window (made once, placed, not focused, hidden with its
   window); `/__window/quick-look` and `elyra:quick-look`.
3. `quickLook` in `@elyra/runtime`; `<Table preview>`.
4. `make:resource --view`'s `preview`; docs (`docs/windows.md`,
   `docs/ui/table.md`, `docs/frontend-runtime.md`).

## Not in this RFC

- **Previewing files** (a PDF, an image) as macOS's own Quick Look does:
  the OS can, and `shell.openExternal` or RFC 0038's editing open them.
- **Several previews at once**, pinned side by side.
- **Previews from other lists than `<Table>`** (a `TreeView`, a custom
  list): `quickLook.show` is there for them; wiring their keys is theirs.

## Alternatives considered

- **A modal or a drawer over the list.** It hides the list it's previewing,
  and a drawer's width fights the table's.
- **A split view in the same window** (`SplitPane`): good for a master-detail
  screen designed as one, and the app can already build it. Quick Look is
  for lists that weren't.
- **Read-only by convention** (`only_abilities(["*.view"])`): it depends on
  how abilities are named, and a `*.view` command that writes would slip
  through. Live-ness, and the write guard live queries already run under,
  is something the framework enforces.

## As built

- **`WindowConfig::read_only()`** keeps an allowlist of the routes that
  read (`security::read_only_allows`), not a list of those that write: a
  new route is refused there until it's added. The store's `get`/`all`,
  the cache's `get`/`has`, storage's reads, `paths`, the update check,
  the backend's lists and state (`state`, `accounts`, `abilities`,
  `connection`, `outbox`, `transfers`, `notifications`, `pending`,
  `editing`), the assistant, presence and listening; and the routes no
  capability guards. (0.50.1: the assistant without its `key`, presence
  and listening only `join` and `leave`, the routes no capability guards
  one by one, without the locale's choice; a cancel is the window's own.) Commands must be live; a call from the window runs
  inside `elyra_db::live::track(Some(name), ..)`, the guard a live re-run
  has, with the `database` feature (without it there are no live
  commands, so none run).
- **`can`** gets the abilities only non-live commands need as the
  window's `deny`; the guard refuses those commands anyway.
- **The assistant** offers a window that only reads only tools marked
  read-only (its live commands), and refuses the others there.
- **The window** is made the first time with `build_window`, its route as
  `#/…`, hidden after; a later show sets `location.hash` in it. It's
  placed with `QuickLookSpot::beside`, shown, and the window it came from
  is focused again (tao shows a window by making it key). Its close
  button hides it; the window it was shown beside closing hides it; it
  isn't counted when the app decides whether its last window closed.
  **The app going to the background hides it**: when the app's last
  focused window loses the focus, the loop asks again 300 ms later (a
  window losing it to another of the app's is followed by that one getting
  it, not always in the same turn), and hides it if none has it back.
- **`quickLook.show(route)`** is `/__window/quick-look-show`; the shell
  takes only an app route (`/…` or `#/…`, no `//`, backslash or newline,
  at most 2,000 characters). `elyra:quick-look` says what's shown.
- **Tested**: `tests/read_only_window.rs` (live commands only, a live
  command that writes refused at the write, the routes, `can`), the
  route rule and the placement as unit tests, `<Table preview>`'s keys
  (4) and `quickLook` (3) in the frontend, `make:resource --view`'s
  `preview`. **Tried on macOS** in the example app, driven through `rata
  mcp dev`: Space on a todo showed its route beside the window, the list
  kept the focus, ↓ moved it to the next row's route, and Esc hid it. That
  found two things, fixed: a window macOS has never shown isn't moved or
  sized, so it's shown first; and it's as tall as its window's inner
  height, not its outer one. Not yet tried on Windows and Linux.

## Decisions

Settled on 2026-11-20, all as recommended:

1. **Space previews** without `select`; with `select`, **⌘Y / Ctrl+Y**.
2. **With `preview`, Enter opens and Space previews**; without it, nothing
   changes.
3. **`read_only()` denies everything that changes something**: non-live
   commands, live ones' writes (under the live write guard), and the
   capabilities listed above.
4. **Beside its window**, over it when the screen has no room.
5. **`make:resource --view` writes `preview`** to the record's own route.
