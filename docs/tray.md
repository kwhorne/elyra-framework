# System tray

Feature-gated behind `tray` (tao dropped its built-in tray in 0.35, so this
wraps the `tray-icon` crate).

```toml
elyra = { version = "0.1", features = ["tray"] }
```

## Configuring the tray

```rust
use elyra::TrayConfig;

App::new()
    .tray(
        TrayConfig::new()
            .tooltip("My App")
            .title("My App")            // short text next to the icon (macOS/Linux)
            .item("open", "Open")       // custom item -> emits "open" on the "tray" channel
            .separator()
            .quit("Quit"),              // closes the app
    )
    .run();
```

Builder: `new()`, `tooltip`, `title`, `item(id, label)`, `separator()`,
`quit(label)`, and [`popover(window)`](#a-window-that-opens-from-the-icon). A
simple solid-color icon is generated, so no image asset is required.

## Handling clicks (frontend)

A custom item click emits its `id` on the `"tray"` [event channel](events.md); a
`Quit` item closes the app.

```svelte
<script>
  import { channel } from "@elyra/runtime";
  const tray = channel("tray");   // e.g. "open"
  $effect(() => { if ($tray === "open") { /* focus / navigate */ } });
</script>
```

## A window that opens from the icon

A popover ([RFC 0019](proposals/0019-tray-popover.md)): a small window under
the tray icon for what's asked at a glance (today's hours, a quick note, what's
due). A left click on the icon shows it; a click elsewhere, Esc, or another
click on the icon hides it. A right click shows the menu, as before.

```rust
use elyra::{TrayConfig, WindowConfig};

App::new()
    .tray(
        TrayConfig::new()
            .tooltip("Timer")
            .popover(
                WindowConfig::new("quick")
                    .title("Timer")                  // its item in Linux's menu
                    .path("#/quick")                 // a route of the app's router
                    .size(320.0, 420.0)
                    .only_abilities(["time.*"]),     // only what it needs
            )
            .item("open", "Open Timer")
            .quit("Quit"),
    )
    .run();
```

- **Placed by the icon:** below it in a top bar (macOS's menu bar), above it
  with the taskbar at the bottom, beside it with the taskbar at a side, and
  kept on the monitor.
- **A window like any other:** the app's own page at `path`, scoped as
  [any window](windows.md#giving-a-window-less). It's over every other app
  while it's shown, so give it only what it needs.
- **Made at start, hidden,** so the first click shows it at once, loaded. It
  hides rather than closes; its page stays, and its live queries keep
  running, so it's current when shown. It doesn't keep the app running: the
  app ends with its last other window.
- **Undecorated, on top, out of the taskbar.** It draws its own frame.
- **On macOS** showing it activates the app without bringing its other
  windows forward.
- **On Linux** the icon sends no clicks (AppIndicator), so the tray's menu
  gets an item at the top, the window's title, that shows it: by the cursor
  on X11, at the top of the screen on Wayland.

From code, for a global shortcut say:

```rust
ctx.get::<Windows>().toggle_popover();   // or show_popover(), hide_popover()
```

Without an icon click it opens where it last opened, or by the cursor.

In the page, `appWindow.onState` says when it's shown, to refresh or focus
an input:

```ts
import { appWindow } from "@elyra/runtime";

appWindow.onState((s) => {
  if (s.label === "quick" && s.visible && s.focused) input.focus();
});
```

Esc hides it unless something on the page handled the key first (a dialog,
an open list): those call `preventDefault()` or stop it.

It doesn't open over a full-screen app on macOS (it opens in the current
space), and there's one popover, for the one tray icon.

## An app that stays in the tray

For an app whose home is the icon (a timer, a sync client, an inbox watched
all day), `stay_in_tray()` ([RFC 0020](proposals/0020-stay-in-tray.md)):

```rust
App::new()
    .title("Timer")
    .stay_in_tray()
    .tray(
        TrayConfig::new()
            .tooltip("Timer")
            .popover(WindowConfig::new("quick").path("#/quick").size(320.0, 420.0))
            .item("open", "Open Timer")
            .quit("Quit Timer"),
    )
    .run();
```

```svelte
<!-- the main window's page: hidden, but loaded -->
<script>
  import { appWindow, channel } from "@elyra/runtime";
  const tray = channel("tray");
  $effect(() => { if ($tray === "open") appWindow.show(); });
</script>
```

- **It starts with the icon only.** Its windows are made, hidden, so
  showing one is at once.
- **Closing the main window hides it,** its page and live queries kept;
  `appWindow.close()` on it does the same. Other windows close as before.
- **It runs until a quit:** the tray's Quit item, ⌘Q, or `quit()` from the
  runtime (`Windows::quit()` in Rust), under the window capability.
- **Showing a window brings it in front** of the app in front.
- **On macOS it's out of the Dock** (and ⌘Tab, and the menu bar) while no
  window is shown, and in it while one is. The popover doesn't count.
  `rata bundle` asks the app, and writes `LSUIElement` into the `.app` so
  its Dock icon doesn't show for a moment at launch.
- **It's a single instance:** opening the app again (a second launch, or
  Finder, Spotlight or the Dock on macOS) shows the main window.
- **It needs a tray:** without `.tray(..)` it doesn't start. If the icon
  can't be made, the app falls back to an ordinary one: the main window
  shows, and the app ends when it closes.

**GNOME** shows no tray icons without an AppIndicator extension, and the app
can't tell: there, opening the app again from the launcher is the way back
to its window. Say so in your app's docs, or offer the window on first run.

## Notes

- The tray is created after the event loop initializes (required on macOS) and
  held for the program's lifetime.
- Menu clicks are routed through the event loop and re-emitted on the bus, so
  your frontend reacts the same way it does to any other event.

## Related

- [Events](events.md) · [Windows](windows.md)
