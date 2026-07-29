# System integration

Feature-gated behind `system`. The native desktop capabilities almost every app
needs — file dialogs, opening things in the OS, the clipboard, notifications,
and standard paths — exposed to the frontend through `@elyra/runtime`.

```toml
elyra = { version = "0.5", features = ["system"] }
```

No Rust wiring is required: enabling the feature adds the shell's `/__sys/*`
endpoints. Everything is driven from the frontend.

## Frontend API (`@elyra/runtime`)

```ts
import { dialog, shell, clipboard, notify, paths } from "@elyra/runtime";

// File dialogs — resolve to absolute path strings.
const files = await dialog.open({
  title: "Choose images",
  multiple: true,
  filters: [{ name: "Images", extensions: ["png", "jpg", "webp"] }],
});
const dir = await dialog.open({ directory: true });        // string[]
const target = await dialog.save({ defaultName: "export.json" }); // string | null

// Open a URL or path with the OS default handler.
await shell.openExternal("https://elyracode.com/framework");

// Clipboard (text).
await clipboard.writeText("copied!");
const text = await clipboard.readText();

// OS notification.
await notify("Done", "Your export finished.");

// Standard OS directories + the running executable.
const p = await paths(); // { home, config, data, cache, temp, exe }
```

`dialog.open` always returns an array (empty when cancelled); pass
`multiple: false` (the default) and read `[0]`. `dialog.save` returns the path
or `null`.

## Rust API

The same operations are available directly (e.g. from a `#[command]`) via the
[`system`](../framework/src/system.rs) module:

```rust
use elyra::system;

let picked = system::open_dialog(system::OpenDialog { multiple: true, ..Default::default() }).await;
system::open_external("https://example.com")?;
system::clipboard_write("hi")?;
let text = system::clipboard_read()?;
system::notify(system::Notification { title: "Hi".into(), body: None })?;
let paths = system::paths();
```

## `shell.openExternal` is policy-gated

"Open with the OS default handler" must not become "run this program", so the
shell validates the target before handing it over:

- **Allowed schemes:** `http`, `https`, `mailto`. Widen with
  `App::allow_open_schemes(["slack", "zoommtg"])`.
- **`file:` URLs** are refused — pass an absolute path instead.
- **Paths** must be absolute and exist, and anything executable is refused
  (`.exe`, `.sh`, `.app`, `.desktop`, `.jar`, … anywhere in the path). Disable
  path opening entirely with `App::deny_open_paths()`.

A blocked call rejects with a `… is not allowed by policy` message rather than
silently doing nothing. See [`elyra::security`] for the full policy surface.

[`elyra::security`]: https://docs.rs/elyra/latest/elyra/security/index.html

## Notes & platform caveats

- **File dialogs** use `rfd`'s async API, which marshals to the platform's main
  thread internally, so they are safe to call from Elyra's tokio-driven IPC.
- **Notifications** on macOS are only delivered from a **bundled** app (one with
  a bundle identifier); un-bundled `cargo run` may show nothing.
- **Linux** builds of the `system` feature need GTK + a clipboard backend
  (`libgtk-3-dev`, X11/Wayland libs) present at build time, like any native
  dialog/clipboard stack.
- Errors (e.g. a clipboard failure) reject the returned promise.

## Related

- [Frontend runtime](frontend-runtime.md) · [Windows](windows.md) · [Tray](tray.md)
