# RFC 0028 — The tray's icon, and the app's

**Status:** implemented (2026-11-07) · **Target:** 0.39.0 · builds on [RFC 0008](0008-laravel-notifications.md), [RFC 0019](0019-tray-popover.md), [RFC 0020](0020-stay-in-tray.md) and [RFC 0025](0025-commands-and-configuration.md) · see [Decisions](#decisions)

## Summary

An Elyra app's tray icon is a blue square, always: there's no way to give
it the app's own, to let macOS draw it in the menu bar's colour, or to
change it while the app runs. An app that lives in the tray (RFC 0020)
is its icon, so this is the first thing its users see. And when such an
app has closed its windows, its unread count has nowhere to show.

```rust
App::new()
    .icon(Icon::png(include_bytes!("../icons/app.png")))
    .tray(
        TrayConfig::new()
            .icon(Icon::png(include_bytes!("../icons/tray.png")))
            .template()                                  // macOS draws it in the bar's colour
            .icon_named("syncing", Icon::png(include_bytes!("../icons/syncing.png"))),
    )
```

```rust
#[command]
async fn start_timer(ctx: Ctx) {
    let tray = ctx.get::<Tray>();
    tray.set_title(Some("00:00"));                       // beside the icon
    tray.set_icon("syncing");
}
```

## Motivation

- **The icon is the app.** A timer, a sync client, a clipboard manager:
  for apps that stay in the tray, the icon is all there is most of the
  day. A blue square says "unfinished".
- **The menu bar has its own look.** macOS draws its own icons in black on
  a light bar and white on a dark one. An icon that doesn't follow looks
  pasted on. AppKit does it for a *template* image; `tray-icon` can mark
  one, Elyra just never asked.
- **State lives in the tray.** A timer shows `01:24`, a sync client its
  progress, a mail client that something's unread. Today the title is set
  once, at start.
- **Unread with no window.** `Windows::set_badge` (RFC 0008) sets the Dock
  badge through a window on macOS and Linux. An app in the tray with its
  windows closed has none, and the Dock icon is hidden anyway; the count
  reached only the tooltip.
- **Windows and Linux windows have no icon.** The title bar, the taskbar
  and the window switcher show a generic one: `with_window_icon` is never
  called.

## Current state (0.38)

- `tray::build` always calls `default_icon()`, a 32×32 square in
  `#3B82F6`. `TrayConfig` has `tooltip`, `title`, `items` and `popover`.
- The tray handle (`tray_icon::TrayIcon`) lives in the event loop. Nothing
  outside it can reach it; the badge changes its tooltip only.
- `UserEvent::Badge(count)`: the Dock label through the first window
  (macOS), the launcher count (Linux), an overlay on each window's taskbar
  button (Windows), and `"<tooltip> — n unread"` on the tray.
- `rata bundle` makes a `.icns` for the `.app` and puts the icon in the
  `.deb`; the Windows folder gets the source image copied beside the
  `.exe`, which Explorer doesn't use.

## Design

### `Icon`

`elyra::Icon` is an image read once, when the app is built:
`Icon::png(&[u8])` (any colour type and depth, normalised to 8-bit RGBA)
or `Icon::rgba(pixels, w, h)`. A PNG that can't be read isn't a panic at
the call: it carries its problem, and `App::problems()` (RFC 0025) lists
it, so `App::run` stops before anything starts, with every problem at once.

### The tray's icon

- `TrayConfig::icon(Icon)`: its icon. Without one, the app's
  (`App::icon`), else the blue square, now with rounded corners.
- `TrayConfig::template()`: on macOS the icon is a template, drawn by the
  system from its alpha in the menu bar's colour. Elsewhere it's drawn as
  it is.
- `TrayConfig::icon_named(name, Icon)`: other icons, shown by name while
  the app runs. Given twice under one name is a problem.

### `Tray`, while the app runs

`ctx.get::<Tray>()` (bound when the app has a tray, with the `tray`
feature) sends changes to the main thread as `Windows` does:

| | |
|---|---|
| `set_title(Option<&str>)` | the text beside the icon (macOS, Linux) |
| `set_tooltip(Option<&str>)` | the tooltip; `None` puts back the config's, else the app's name |
| `set_icon(name)` | a named icon; a name it wasn't given is refused (`false`, and a warning), and the icon stays |
| `reset_icon()` | the tray's own icon again |

The event loop keeps what the tray shows (`tray::Live`): the config's
icons and what `Tray` and the unread count have changed, and applies all
of it after each change. That keeps the combining rules in one tested
place, away from the native calls.

### Unread on the tray

By default the unread count shows on the tray too, so an app in the tray
with no window open still shows it:

- **macOS, Linux:** after the title, `01:24 · 3`, or `3` alone. The text
  is drawn by the system, so it works with a template icon.
- **Windows:** the tray has no text; the icon gets a red dot in its
  lower-right corner, with a clear ring around it.
- **Everywhere:** `"<tooltip> — 3 unread"`, as before.

`TrayConfig::no_unread()` leaves it off: a timer whose title is the time,
say, with the count shown in the app.

### The app's icon

`App::icon(Icon)` sets every window's icon on Windows (title bar, taskbar
button, Alt-Tab) and Linux (the window switcher, where the `.desktop`
entry doesn't give one). macOS has no window icons; there the Dock's comes
from the bundle (`rata bundle`'s `.icns`), and `App::icon` is only the
tray's fallback.

## Not in this RFC

- **The `.exe`'s own icon** (Explorer, the Start menu): it's a resource
  linked into the binary, so a build script with a resource compiler
  (`embed-resource` and `rc.exe` or `windres`). It belongs with the
  Windows installer work (MSI/NSIS), not the runtime.
- **The tray from the frontend.** `Tray` is Rust-only. A command can
  call it; a page-side facade, with its capability, can follow if apps
  ask.
- **Animated icons** (a spinner while syncing): a timer switching named
  icons does it today; a frame list can come later.

## Testing

- `icon.rs`: PNGs of every colour type and of 16 bits read as RGBA;
  what isn't a PNG, and RGBA of the wrong length, are problems; the dot
  and the fallback.
- `tray.rs`: which icon the tray takes; a named icon until reset; the
  count after the title on macOS and Linux and as a dot on Windows; the
  tooltip's order; `no_unread`; `Tray` sends its changes and refuses an
  unknown name; unreadable and doubly named icons are problems.
- By hand on macOS: the example app's template icon in a dark menu bar,
  and its title changed by a command (`5 todos` after adding one).

## Decisions

1. **One `Icon` type, read once.** PNG only: it's what apps ship, and
   the `png` crate is already in the tree. SVG would bring a renderer.
2. **A bad icon is a problem, not a panic or a log line.** It's
   configuration (RFC 0025): found before anything starts, with the rest.
3. **`Tray` in the container, not a facade.** Tray state is app state:
   a timer, a sync, the unread count. The frontend reaches it through a
   command.
4. **Named icons, not bytes at runtime.** Icons are read and checked at
   build time; `set_icon("syncing")` can't fail on a broken image later.
5. **Unread on the tray by default.** Without it an app in the tray loses
   the count when its windows close, which is the case RFC 0020 is for.
   `no_unread()` for the apps whose title says something else.
6. **A dot on Windows, the number elsewhere.** A digit in a 16 px tray
   icon can't be read; macOS and Linux draw text beside the icon, and the
   number is what users look for there.
