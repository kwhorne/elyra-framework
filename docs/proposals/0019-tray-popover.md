# RFC 0019 — A window that opens from the tray

**Status:** proposed (2026-11-03) · **Target:** 0.32.0 · builds on [RFC 0016](0016-window-capabilities.md) · see [Open questions](#open-questions)

## Summary

A small window that opens under the tray icon when it's clicked, and goes
away when the user clicks elsewhere: today's hours, a quick note, the next
three invoices due. The app's own page, in a window given only what it
needs:

```rust
App::new()
    .tray(
        TrayConfig::new()
            .tooltip("Timer")
            .popover(
                WindowConfig::new("quick")
                    .path("quick")
                    .size(320.0, 420.0)
                    .only_abilities(["time.*"]),
            )
            .item("open", "Open Timer")
            .quit("Quit"),
    )
    .run();
```

A left click on the icon shows `quick` under it (above it, with the
taskbar at the bottom); a click elsewhere, Esc, or another click on the
icon hides it. A right click shows the menu, as now.

## Motivation

- **A glance without the app.** Much of what a business app is asked is
  small: is the timer running, what's due today, jot this down. Bringing
  up the main window for it is a context switch; a menu can't show it.
- **The pieces are there.** The tray (`tray-icon`) and windows (tao) are
  in place, and since RFC 0016 a second window can be given less than the
  app grants: a window over every other app that can only start and stop
  a timer is a safe one to have.
- **Every app writes it badly.** Placing a window by the icon on each
  platform, hiding it on blur without the click on the icon reopening it,
  keeping it out of the taskbar: small, fiddly, and the same every time.

## Current state (0.31)

- **The tray** (`tray.rs`, `tray` feature): `TrayConfig` with a tooltip, a
  title and a menu of items, a separator and Quit. An item emits its id on
  the `"tray"` channel. The icon has its menu on both clicks; clicks on
  the icon itself aren't listened for.
- **Windows** (`window.rs`): `WindowConfig` with size, decorations,
  `always_on_top`, a path, and its scope (RFC 0016). `Windows` opens,
  shows, hides, focuses, closes. The app exits when its last window
  closes. `elyra:window` tells pages a window's size and focus.
- **`tray-icon` 0.24** sends `TrayIconEvent::Click` with the icon's rect
  on macOS and Windows, and can keep the menu off the left click
  (`with_menu_on_left_click(false)`). **On Linux it sends no click events
  at all** (AppIndicator), and has no rect: the icon only has its menu.
- **tao 0.35** has what placing needs: the monitor at a point, the
  cursor's position (not on Wayland), a hidden window, skip-taskbar on
  Windows and Linux.

## Design

### `TrayConfig::popover(WindowConfig)`

The tray has at most one popover, a window like any other: its own label,
path, size and scope. What `popover` sets on it:

- **No decorations, not resizable, always on top, out of the taskbar**
  (Windows, Linux), with a shadow (macOS).
- **Made hidden at start,** so the first click shows it at once, loaded
  (open question 2).
- **Hides, not closes:** a click elsewhere (it loses focus), Esc (in the
  runtime), or a click on the icon. Its page stays loaded, its live
  queries keep running, so it's current when shown.
- **Doesn't keep the app alive:** when the last other window closes, the
  app exits as now (open question 5).

On macOS and Windows a popover takes the left click and leaves the menu
to the right click (`with_menu_on_left_click(false)`). Without a popover,
nothing changes.

### Where it opens

A pure function places it, so it's tested on every platform:

```rust
fn place(icon: Rect, size: Size, monitor: Rect) -> Position
```

- **By the nearest edge of the monitor to the icon:** at the top (macOS's
  menu bar, a top panel) it opens below the icon; at the bottom (the
  Windows taskbar) above it; at the left or right (a vertical taskbar)
  beside it. 4 px from the icon.
- **Centered on the icon** along that edge, then **kept inside the
  monitor** with an 8 px margin.
- In physical pixels, as `tray-icon` gives the rect, on the monitor the
  icon is on.

### Showing and hiding

- **A click on the icon** (left button, on release) shows it if hidden,
  and hides it if shown.
- **Losing focus hides it.** A click on the icon while it's shown first
  takes its focus, which hides it; the click itself then mustn't show it
  again. So a click within 300 ms of a hide by blur does nothing.
- **Shown, it takes focus,** so typing goes to it, and Esc and a click
  elsewhere work. On macOS tao's `set_focus` activates the app
  (`activateIgnoringOtherApps`); if that brings the main window forward
  too, the popover is shown without activating the rest, and the build
  says which it took.
- **From code:** `Windows::show_popover()`, `hide_popover()` and
  `toggle_popover()`, for a global shortcut (`App::global_shortcut`), say.
  Without an icon click, it opens where it last
  opened, or by the cursor, or centered at the top of the main monitor.

### On Linux

No clicks reach the app (above), so on Linux the popover opens **from the
menu**: `popover` adds an item at the top of the tray's menu, the
window's title ("Timer"), that shows it. It opens by the cursor on X11,
which is by the icon a moment ago; on Wayland, where a window can't place
itself, centered at the top. It hides on blur and Esc as elsewhere
(open question 3).

### In the page

- **`elyra:window`** says whether the window is `visible`, and is sent
  when the popover is shown or hidden: a page refreshes, or focuses its
  input, on `appWindow.onState(..)`.
- **Esc** in a popover hides it (the runtime, unless the page handled
  it).
- `appWindow.hide()` hides it, as for any window.

### Security

- **Nothing new is reached:** the popover is a window, and RFC 0016
  applies. It asks through its own protocol handler, as itself, within
  its scope; the docs' example narrows it, and the guide says to.
- **It's over every other app.** It shows only when the user clicks the
  icon or the app calls `show_popover`, which an app can do for any
  window today, and hides on the first click elsewhere.

### What it doesn't do

- **A menu-bar-only app** (no main window, no dock icon): a popover is
  how one would look, but the app's lifetime and activation policy are
  their own question (open question 5).
- **Over a full-screen app on macOS:** that takes a window collection
  behavior tao doesn't set. It opens in the current space, not over a
  full-screen one.
- **More than one tray icon or popover.**
- **An arrow pointing at the icon,** or a translucent background: the
  page draws its own frame.

### Testing

- **`place`:** the four edges, a corner where it must be pushed inside, a
  monitor left of or above the main one (negative coordinates), and a
  window bigger than the room on that side.
- **Showing and hiding** as a small state machine, with the clock given:
  click shows, click hides, blur hides, a click soon after a blur doesn't
  reopen, one later does; `show_popover` with no rect yet.
- **The config:** a popover's window is undecorated, on top, hidden at
  start, out of the taskbar; the menu loses the left click; on Linux the
  menu gains the item.
- **The runtime:** Esc hides a popover and not another window;
  `visible` in `elyra:window`.
- **The platforms:** CI's matrix compiles the macOS, Windows and Linux
  code; the example app gets a popover, launch-smoked on macOS, with what
  isn't seen on Windows and Linux called out as unverified.

## Implementation plan

1. `TrayConfig::popover`, `place` and the show/hide state machine, with
   their tests.
2. The event loop: tray clicks, the popover's window, blur, the Linux
   menu item; `Windows::show_popover` and the others.
3. The runtime: `visible` in `elyra:window`, Esc.
4. The example app's popover; docs: the tray guide, windows, the
   changelog.

## Alternatives considered

- **`WindowConfig::tray_popover()`** (as first sketched): a flag on any
  window. But the popover needs the tray's click and its rect, and there's
  one tray; on the tray's config it can't be set on two windows, or
  without a tray.
- **A native popover** (`NSPopover` on macOS, a flyout on Windows): the
  platform's look, with an arrow, but a webview inside a native popover
  per platform, three times over, and nothing on Linux.
- **Leave it to apps:** `TrayIconEvent` re-emitted on the bus, and a
  window the app places itself. The fiddly parts (the blur-then-click
  race, the taskbar's edge, keeping it on the monitor) are the whole job.

## Open questions

1. **On `TrayConfig` or `WindowConfig`?** `TrayConfig::popover(window)`
   (recommended: one tray, one popover, and its click) or
   `WindowConfig::tray_popover()`.
2. **Made at start, hidden, or on the first click?** At start
   (recommended): the first click shows it at once. It costs a webview
   for the run (tens of MB), as any open window does. On the first click
   it costs a moment's blank window, each run.
3. **On Linux:** a menu item that opens it, by the cursor
   (recommended), or no popover on Linux (its items only)?
4. **Live queries while it's hidden:** keep running (recommended; it's
   current when shown, at the cost of an open window), or paused until
   shown?
5. **Does the popover keep the app running** when the main window closes?
   No (recommended): as now, the app ends with its last ordinary window.
   A menu-bar-only app, where the tray is the app, is its own RFC: its
   activation policy on macOS, how it's quit, and how the main window
   comes back.
6. **The hide-on-blur window:** 300 ms (recommended), or follow the
   platform's double-click time?
