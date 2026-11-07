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
`quit(label)`, [`popover(window)`](#a-window-that-opens-from-the-icon), and
[`icon`, `template`, `icon_named`, `no_unread`](#the-icon). Without an
icon, the tray takes the app's ([`App::icon`](windows.md#the-apps-icon)),
else a blue square, so no image asset is required.

## The icon

Give the tray its own icon, a PNG, read when the app is built
([RFC 0028](proposals/0028-tray-and-icons.md)):

```rust
use elyra::{Icon, TrayConfig};

TrayConfig::new()
    .icon(Icon::png(include_bytes!("../icons/tray.png")))
    .template()
    .icon_named("syncing", Icon::png(include_bytes!("../icons/syncing.png")))
```

- **Size:** square, 32 px or more; 44 px is sharp in a Retina menu bar
  (it's drawn at 22 pt).
- **`template()`:** macOS draws the icon in the menu bar's colour, black
  on a light bar and white on a dark one, like the system's own. Draw it
  black on transparency; only its alpha counts. On Windows and Linux the
  icon is drawn as it is, so pick one that reads on both a light and a
  dark taskbar, or skip `template()` and use colour everywhere.
- **`icon_named(name, icon)`:** other icons, to show while the app runs
  (below). Drawn as templates too when the tray's icon is.

A PNG that can't be read, or two icons with one name, is a
[configuration problem](configuration.md): `App::run` stops before
anything starts and lists it with the rest.

## Changing it while the app runs

`ctx.get::<Tray>()` changes the tray from a command, a provider's task or
anywhere the container reaches (it's bound when the app has a tray):

```rust
use elyra::Tray;

#[command]
async fn start_sync(ctx: Ctx) {
    let tray = ctx.get::<Tray>();
    tray.set_icon("syncing");
    tray.set_tooltip(Some("Syncing…"));
    // … the work …
    tray.reset_icon();
    tray.set_tooltip(None);              // back to the config's, else the app's name
}
```

| | |
|---|---|
| `set_title(Option<&str>)` | the text beside the icon (macOS, Linux; Windows' tray has none) |
| `set_tooltip(Option<&str>)` | the tooltip; `None` puts back the config's, else the app's name |
| `set_icon(name)` | an icon from `icon_named`; a name it wasn't given is refused (`false`) and the icon stays |
| `reset_icon()` | the tray's own icon again |

A timer updates its title from a task, once a second:

```rust
let tray = ctx.get::<Tray>();
tokio::spawn(async move {
    let start = std::time::Instant::now();
    loop {
        let s = start.elapsed().as_secs();
        tray.set_title(Some(&format!("{:02}:{:02}", s / 60, s % 60)));
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
});
```

Use `ctx.try_get::<Tray>()` in code that also runs without a tray (a test
with [`TestApp`](testing.md), an app built without the `tray` feature).

## The unread count

`Windows::set_badge(n)` ([Laravel's notifications](backend.md#laravels-notifications) set it) shows on the tray as well as
the Dock, the launcher or the taskbar, so an app that
[stays in the tray](#an-app-that-stays-in-the-tray) with its windows
closed still shows it:

- **macOS, Linux:** after the title, `01:24 · 3`, or `3` alone (`9+` past
  nine).
- **Windows:** a red dot on the icon's lower-right corner.
- **The tooltip, everywhere:** `My App — 3 unread`.

`TrayConfig::no_unread()` leaves it off the tray: for a timer whose title
is the time, say.

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
