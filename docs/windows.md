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
`decorations`, `always_on_top`, `path`. `path` is appended to the app origin so a
window can open a specific route of your frontend.

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
`window`). Actions target the focused window (or the primary one) and are applied
on the main thread:

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

Native file drops onto the window are delivered to the frontend:

```ts
import { onFileDrop } from "@elyra/runtime";

const off = onFileDrop((paths) => {
  console.log("dropped", paths); // absolute paths
});
```

## Lifecycle

The app exits when the **last** window closes.

## Related

- [Architecture — threads](architecture.md#processes-and-threads)
- [Configuration](configuration.md)
