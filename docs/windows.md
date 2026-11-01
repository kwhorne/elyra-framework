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
`decorations`, `always_on_top`, `path`, and the limits below (`deny`,
`only_capabilities`, `only_abilities`, `deny_abilities`). `path` is appended to
the app origin so a window can open a specific route of your frontend.

## Giving a window less

A window gets what the app grants, unless its `WindowConfig` narrows it
([RFC 0016](proposals/0016-window-capabilities.md)). A preview of a rendered
invoice shows content the app didn't write; if a script got in there, it
should reach only what the preview needs:

```rust
use elyra::security::Capability;

ctx.get::<Windows>().open(
    WindowConfig::new("preview")
        .path("/invoices/42")
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

// Live state (pushed on resize / move / focus):
const off = appWindow.onState((s) => {
  console.log(s.width, s.height, s.maximized, s.fullscreen, s.focused);
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

## Lifecycle

The app exits when the **last** window closes.

## Related

- [Architecture — threads](architecture.md#processes-and-threads)
- [Configuration](configuration.md)
