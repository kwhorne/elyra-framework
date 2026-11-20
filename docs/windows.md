# Windows & multi-window

The primary window is configured via `App` builder methods; additional windows
can be declared at startup or opened at runtime. All windows share one event
loop, one protocol handler, and one event bus (same origin, same commands).

## Primary window

```rust
App::new()
    .title("My App")
    .size(900.0, 640.0)
    .min_size(420.0, 480.0)
    .resizable(true)
    .decorations(true)
    .always_on_top(false)
    .run();
```

## Additional startup windows

```rust
use elyra::WindowConfig;

App::new()
    .window(
        WindowConfig::new("inspector")
            .title("Inspector")
            .size(360.0, 600.0)
            .path("inspector"),        // deep-link into the SPA
    )
    .run();
```

`WindowConfig` builder: `new(label)`, `title`, `size`, `min_size`, `resizable`,
`decorations`, `always_on_top`, `visible` (`false` makes it hidden, for
`Windows::show`), `skip_taskbar` (Windows, Linux), `path`, and the limits below
(`deny`, `only_capabilities`, `only_abilities`, `deny_abilities`).

`path` is appended to the app origin so a window can open a specific route of
your frontend. A path without a file extension (`settings`, `quick`) is
answered with the app's page, as Vite does in `rata dev`, and the page reads
`location.pathname`; with the runtime's [router](frontend-runtime.md#routing), open a
route with `path("#/invoices/42")`.

## Giving a window less

A window gets what the app grants, unless its `WindowConfig` narrows it
([RFC 0016](proposals/0016-window-capabilities.md)). A preview of a rendered
invoice shows content the app didn't write; if a script got in there, it
should reach only what the preview needs:

```rust
use elyra::security::Capability;

ctx.get::<Windows>().open(
    WindowConfig::new("preview")
        .path("#/invoices/42")
        .only_abilities(["invoices.view"])   // no invoices.delete, no billing.*
        .deny(Capability::Assistant),        // no assistant panel here
);
```

- **`only_abilities([..])`** keeps only the abilities the patterns cover,
  and **`deny_abilities([..])`** takes some away. A command whose
  `#[command(can = "…")]` the window doesn't allow is refused (`denied`),
  its live queries too, and `can` says no, so its buttons hide.
- **`deny(Capability::X)`** and **`only_capabilities([..])`** do the same for
  whole routes: the assistant, the store, the sidecar, …
- **A command without `can`** stays reachable wherever the window has
  `Capability::Commands`. Deny `Commands` for a window that must not run
  any, or declare abilities on the commands that matter.
- **It only narrows.** Nothing here gives a window more than the app
  grants, and the signed-in user's abilities still apply.
- **The assistant** in that window is offered only the tools it allows,
  and checks each call again.
- **Commands see it:** `CommandRequest::window` is the label of the window
  a call came from, for a middleware to log or refuse.

The shell knows which window asks by the window's own protocol handler,
not by anything the page sends, so a page can't borrow another window's
limits. A command called from a narrowed window can still open a window
with fewer limits: that's the app's decision, in its own code.

### Only reading

`.read_only()` narrows a window to reading
([RFC 0041](proposals/0041-quick-look.md)): a preview of something someone
else wrote, say.

```rust
WindowConfig::new("preview").path("#/invoices/42").read_only()
```

- **Only live commands run** (`#[command(live)]`), and they run under the
  guard a live query's re-run has, whether subscribed to or called once: a
  model-layer write, or a write to the backend, in one fails with "`x` is
  a live command and must only read". Any other command is refused,
  `denied`, "this window only reads". Raw SQL and a command's own I/O are
  outside that guard, as for a live query.
- **`can` says no** to an ability only commands that may write need, so
  their buttons hide; one a live command needs is as it was.
- **Only the routes that read are there**: the store's `get` and `all`, the
  cache's `get` and `has`, storage's reads, `paths`, the update check, the
  backend's lists and state, the assistant, presence and listening. The
  store's `set`, the clipboard, dialogs, `shell.openExternal`, a sign-out,
  the queue, the sidecar, the microphone, diagnostics and `quit` aren't.
- **The assistant** in that window is offered only live tools.
- **It only narrows**, and goes with `only_abilities` and the rest.

## Quick Look

A record at a glance, from a list, without leaving it
([RFC 0041](proposals/0041-quick-look.md)). `<Table preview>` drives it
from the keyboard ([Table](ui/table.md#quick-look)); from code:

```ts
import { quickLook } from "@elyra/runtime";

await quickLook.show("/customers/42");   // open it beside this window, or move it there
await quickLook.hide();
$quickLook;                              // { open, route }
quickLook.here();                        // true in the Quick Look window itself
```

```rust
ctx.get::<Windows>().show_quick_look("/customers/42");
```

- **One window for the app**, labelled `quick-look`, read-only (above),
  made the first time and hidden after, so the next look is at once. Its
  page is the app's own at the route, which moves when it's shown again.
- **Beside the window it came from**: to its right when the screen has
  room, else its left, else over its right part; as tall as that window,
  a third of its width (at least 360 px). That window keeps the focus.
- **Esc or Space** in it closes it, as its close button does. It goes
  with the window it was shown beside, hides when the app goes to the
  background, and doesn't keep the app running.
- **Only the app's routes**: `/customers/42` or `#/customers/42`, never a
  URL.

## Opening windows at runtime

`App` binds a `Windows` handle into the container. Resolve it anywhere — even
inside a command — to open new windows:

```rust
use elyra::{Windows, WindowConfig};

#[command]
async fn open_settings(ctx: Ctx) {
    ctx.get::<Windows>().open(
        WindowConfig::new("settings").title("Settings").path("settings"),
    );
}
```

`Windows::open` sends a request to the main-thread event loop (via an
`EventLoopProxy`), which builds the window and webview. It returns `false` if the
event loop has already exited.

## Controlling the window from the frontend

`@elyra/runtime` exports `appWindow` (named so it doesn't clash with the global
`window`). Actions target the window they're called from, and are applied on
the main thread:

```ts
import { appWindow } from "@elyra/runtime";

appWindow.minimize();
appWindow.toggleMaximize();
appWindow.toggleFullscreen();
appWindow.setTitle("Untitled — MyApp");
appWindow.setSize(1000, 700);
appWindow.center();
appWindow.hide(); appWindow.show(); appWindow.focus();
appWindow.close(); // exits the app when it's the last window

// Live state (pushed on resize / move / focus, and show / hide):
const off = appWindow.onState((s) => {
  console.log(s.label, s.width, s.height, s.maximized, s.fullscreen, s.focused, s.visible);
});
```

The same operations are available from Rust via the container-bound
[`Windows`](../framework/src/window.rs) handle (e.g. `ctx.get::<Windows>().minimize(None)`),
which also takes an optional window label to target a specific window.

## File drop

Native file drops onto the window are delivered to the frontend, each as a
[`ChosenFile`](system.md#a-file-the-user-chose) a command can take:

```ts
import { onFileDrop } from "@elyra/runtime";

const off = onFileDrop((files) => {
  console.log("dropped", files.map((f) => f.path));
  commands.importNotes({ files }); // `files: Vec<ChosenFile>` in Rust
});
```

## The app's icon

Give the app its icon, a PNG, read when the app is built
([RFC 0028](proposals/0028-tray-and-icons.md)):

```rust
use elyra::Icon;

App::new().icon(Icon::png(include_bytes!("../icons/app.png")))
```

It's every window's icon on Windows (the title bar, the taskbar button,
Alt-Tab) and on Linux (the window switcher). macOS has no window icons:
there the Dock's comes from the bundle (`rata bundle` makes a `.icns` from
`[bundle].icon`), and `App::icon` is only the tray's when the tray has
none of its own. Square, 256 px or more. An image that can't be read is a
[configuration problem](configuration.md), found before anything starts.
On Windows it's also the icon of the app's notifications
([system](system.md#notes--platform-caveats)).

The `.exe`'s own icon, the one Explorer and the Start menu show, is
linked into the binary at build time, and isn't set by this: `rata bundle`
links it from `[bundle].icon` ([Windows: the `.exe` and its
installer](cli.md#windows-the-exe-and-its-installer)).

## Remembering size & position

Opt in and the primary window's size, position, and maximized state are restored
on the next launch:

```rust
App::new().title("MyApp").persist_window_state().run()
```

State is written to a small file under the OS config directory
(`~/Library/Application Support/<app>/` on macOS, `%APPDATA%\<app>\` on Windows,
`$XDG_CONFIG_HOME/<app>/` on Linux), keyed by the [About](about.md) name, and
saved when the window loses focus or closes.

A position is put back only when enough of the window's title bar to drag
it by lands on a screen that's there now. Closed on an external screen that
has since been unplugged, the window opens where the OS places a new one,
at its saved size, instead of off every screen.

## Lifecycle

The app exits when the **last** window closes. The tray's
[popover](tray.md#a-window-that-opens-from-the-icon) doesn't count: it's
hidden, not closed, and the app ends with its last other window.

An app that [stays in the tray](tray.md#an-app-that-stays-in-the-tray)
doesn't end with its windows: closing the main window hides it, and the
app runs until a quit (the tray's Quit item, ⌘Q, or `quit()`):

```ts
import { quit } from "@elyra/runtime";

quit(); // ends the app, as the tray's Quit item does
```

## Related

- [Architecture — threads](architecture.md#processes-and-threads)
- [Configuration](configuration.md)
