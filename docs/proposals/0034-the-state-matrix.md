# RFC 0034 — One screen in several states at once

**Status:** implemented (2026-11-19) · **Target:** 0.49.0 · builds on [RFC 0014](0014-elyra-inspector.md), [RFC 0016](0016-window-capabilities.md) and [RFC 0026](0026-the-running-app-for-coding-agents.md) · see [Decisions](#decisions)

## Summary

A screen is written once and seen many ways: dark and light, with more
contrast, narrow, by a user who may delete and by one who may not. Today
the developer sees one of those at a time and clicks through the others,
or doesn't. This opens the screen in all of them at once, side by side,
in a debug build, and every one of them follows the code as it changes.

```rust
App::new()
    .matrix(|m| {
        m.frame("Admin");                                   // as the app is
        m.frame("Can't delete").light().deny_abilities(["*.delete"]);
        m.frame("More contrast").contrast();
        m.frame("Narrow").size(380, 720);
    })
```

**⌘⌥⇧E** (Ctrl+Alt+Shift+E), the inspector's **Matrix** button or `rata dev
--matrix` opens the frames on the route the window shows. Change a line
in the `.svelte` file, and Vite updates every frame at once.

## Motivation

- **The states are already there, one at a time.** The inspector takes an
  ability away (RFC 0014) and unplugs the backend; a window can be given
  less (RFC 0016); `tokens.css` has dark, light and more contrast. Each is
  a switch, and seeing a change in all of them means flipping each switch
  after each edit.
- **What breaks is what isn't looked at.** A button that should hide for a
  user without `customers.delete`, text that fails the contrast in light,
  a toolbar that wraps at 400 px. Each is found by a user, because the
  developer was looking at the admin's dark window.
- **A coding agent sees one window.** `rata mcp dev` gives it a snapshot
  and a picture of the window it works in (RFC 0026). It doesn't know the
  change it made hid the "New" button for every user who isn't an admin.

## Current state (0.48)

- **The inspector's mask** takes abilities away for the whole app, until
  it quits. **Simulate offline** unplugs the backend for the whole app.
- **A window's limits** (`deny_abilities`, `only_abilities`, `deny`) are
  its own, enforced by the shell for the window's own protocol handler, so
  two windows of one app can already be two different users' views.
- **The theme** is the app's: `data-theme` and `data-contrast` on `<html>`,
  set by the template's `theme.js`. Nothing sets it per window.
- **Vite's HMR** reaches every webview that loaded the dev server, so two
  windows on one route already update together.

## Design

### A frame is a window

Each frame is a real window of the app, opened by the shell with a label
`matrix:<name>`, the route of the window it was opened from, and its
frame's settings. Nothing about the page is pretended:

- **Abilities** are the window's limits (RFC 0016). A frame with
  `deny_abilities(["*.delete"])` has its delete commands refused by the
  shell, its `can` saying no and its buttons hidden, as a narrowed window's
  are. It only narrows: a frame can't have more than the app grants and the
  user has.
- **The look** is set on `<html>` before the app's script runs:
  `data-theme` (`dark`, `light`) and `data-contrast="more"`. A frame keeps
  its look: `@elyra/runtime` puts the attributes back if the app's theme
  code changes them, and says so once in the console.
- **The size** is the window's inner size in CSS pixels. Without one, the
  frames share the screen.
- **The data** is the app's own. A frame reads what the app reads, and a
  write in a frame is a real write: the matrix is for looking, and
  `deny_abilities` is how a frame is kept from writing what matters.

Because a frame is a window, everything that works on a window works on
it: the inspector, `window_snapshot`, `window_screenshot`, `app_calls`.

### Opening it

- **⌘⌥⇧E** (Ctrl+Alt+Shift+E), or **Matrix** in the inspector's header, opens the
  frames on the focused window's route, and closes them when they're open.
- **`rata dev --matrix`** opens them once the app's first window has
  loaded.
- **The frames are laid out** in a grid on the screen the window is on,
  in the order they were declared, each at its size or an even share.
  They're windows, so the developer can move them.
- **They follow the route.** When the window they were opened from
  navigates, every frame goes to the same route. A frame can be clicked
  in without that changing the others.

### Without `App::matrix`

An app that declares no frames gets four, from what it already has:

| Frame | |
|---|---|
| **As it is** | the app's own look |
| **Light** | `data-theme="light"` |
| **More contrast** | `data-contrast="more"`, in the app's look |
| **Without abilities** | `deny_abilities(["*"])`: what a user who may do nothing sees |

An app with no abilities gets no fourth frame.

### What stays the whole app's

- **The inspector's mask** applies to every frame, as it does to every
  window. A frame narrows further.
- **Simulate offline** unplugs the backend for the app, so every frame
  goes offline together, and the outbox is the app's. The StatusBar in
  each frame says **Offline (simulated)**. Seeing all the frames offline
  at once is the point: a frame of its own that alone is offline would
  need a second backend, a second outbox and a second connection, and
  would show a state the app can't be in.

### For a coding agent

`rata mcp dev` gets one tool:

| Tool | What it does |
| --- | --- |
| `matrix_screenshot` | one PNG with every frame, each under its name, at most 1920 CSS pixels wide; the matrix is opened for it if it isn't |

And `app_windows` lists the frames with their names and settings, so an
agent can take a frame's `window_snapshot` and see, for instance, that
"New customer" isn't in the tree for "Can't delete". The project's
`ratatosk/ai/guidelines.md` tells agents to take a `matrix_screenshot`
after changing a screen's layout or what it shows by ability.

### Security

- **Debug builds only.** `App::matrix`, the key, the inspector's button
  and `--matrix` are behind `#[cfg(debug_assertions)]`, as the inspector
  is. In a release `App::matrix` is a no-op, and `tests/inspector_release.rs`
  checks that no frame opens.
- **A frame only narrows.** Its abilities are a window's limits, with
  everything RFC 0016 enforces; its look is two attributes on `<html>`.
- **Nothing new reaches the page.** A frame's settings are given to its
  own window's script at start; another window can't read or change them.

### Testing

- **Rust:** the frames' windows are opened with their limits and labels;
  the default four; a frame without abilities is refused a command with
  `can`; frames follow a route change; nothing opens in a release build.
- **Runtime:** a frame's look is on `<html>` before the app's script, and
  put back after the app changes it.
- **`rata`:** `--matrix` opens the frames; `matrix_screenshot` composes
  them, under the test harness's pictures.
- **The example app** declares three frames, and the smoke run opens them.

## Implementation plan

1. `MatrixConfig` and `App::matrix`, the default frames, and opening them
   as windows with their limits, look and size (debug only).
2. The look in `@elyra/runtime`, kept on `<html>`; the template's
   `theme.js` leaves a frame's look alone.
3. Following the route: the runtime reports the route of the window the
   frames came from, and the shell sends it on to them.
4. The key, the inspector's button, `rata dev --matrix`.
5. `matrix_screenshot` and the frames in `app_windows`; the guidelines.
6. Docs: `docs/inspector.md` (a section), `docs/ai-agents.md`, the example
   app.

## As built

- **`App::matrix(|m| …)`** with `Matrix::frame(name)` and `Frame`'s
  `.dark()`, `.light()`, `.contrast()`, `.size(w, h)`,
  `.deny_abilities(..)` and `.only_abilities(..)`, in
  `elyra::matrix`. In a release it keeps nothing.
- **`/__inspector/matrix`** `{ route }` opens or closes the frames: the
  event loop closes them if any is open, else lays them out on the screen
  of the window that asked (under the macOS menu bar, above the Windows
  taskbar) and builds each with `build_window`, its look in an init
  script. **`/__inspector/matrix-route`** `{ route }` is sent on as
  `elyra:matrix-route`, and only a frame acts on it.
- **The look is set twice:** by the init script, so the page never shows
  another first, and by `@elyra/runtime`, which keeps it with a
  `MutationObserver` against the app's theme code. So the template's
  `theme.js` needed no change, and an app made before 0.49 works as it is.
- **The key** works for a script's key event too (no `code`, only `key`),
  which is how `matrix_screenshot` opens the frames: `window_act`'s
  `press` on the focused window.
- **`matrix_screenshot` gives a picture per frame**, each under a line
  with its name and look, at most 640 CSS pixels wide, rather than one
  PNG with every frame. Putting them together in one picture would need
  the framework to draw text, and the agent reads the names better as
  text anyway.
- **Frames don't keep the app running**: closing the last window that
  isn't a frame (or the popover) ends the app.
- **Checked by hand** in the example app on macOS: `rata dev --matrix`
  opened three frames; one edit of `App.svelte` (`can="todos.delete"` on
  the delete button) reached all three through Vite, and the frame
  without `todos.delete` showed none of the five buttons; the frames
  followed `main` to `/todos?x=1`; the key closed them; and
  `matrix_screenshot` opened them again and returned three pictures.

## Not in this RFC

- **A frame alone offline, or with its own outbox.** See
  [What stays the whole app's](#what-stays-the-whole-apps).
- **Another user's data.** A frame is the signed-in user with less, never
  someone else; seeing another user's records is a second sign-in.
- **Variants of a screen side by side** (several `.svelte` drafts of the
  same view, one picked). The matrix shows one screen in several states,
  not several screens.
- **Pointing at a part of a frame and handing it to an agent** with the
  source file and the live queries under it. A frame is a window, so
  that, when it comes, works in frames too.
- **One canvas window** with the frames as child webviews. See
  [Alternatives](#alternatives-considered).

## Alternatives considered

- **One window, the frames as child webviews on a canvas.** Closer to a
  design tool, and pan and zoom come with it. But a child webview isn't a
  window to the shell: it would need its own protocol handler to carry its
  limits, its own picture code on three platforms and its own place in
  `rata mcp dev`. Windows have all of that, and a grid of windows is a
  canvas the OS already draws.
- **A process per frame**, each with its own data folder. Every state
  could be its own, offline and outbox too. But the frames would sign in
  and migrate separately, the single-instance lock would have to step
  aside, and an app with a backend would have several sessions on the
  server. For the states a screen shows, a window is enough.
- **Iframes in one page.** Cheap, and fine for the look and the size, but
  an iframe isn't a window: its calls are its parent's, so it can't have
  fewer abilities.
- **The inspector's switches, one after another** — what there is today.

## Decisions

1. **Four frames by default:** as it is, light, more contrast, and without
   abilities. No narrow frame: what's narrow is the app's to say, since it
   differs between a settings window and a main window.
2. **A write in a frame is real.** A frame that can't write can't show the
   screen a writing user sees; `deny_abilities` is there for a frame that
   mustn't.
3. **⌘⌥⇧E (Ctrl+Alt+Shift+E)**, the inspector's key with Shift. ⌘⌥M would
   read better, but on macOS it's Minimize All.
4. **A frame's title is `<app> — <frame>`**, so the OS's window list and a
   screenshot say which is which.
