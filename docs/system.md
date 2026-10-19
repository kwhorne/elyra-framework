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

// File dialogs — resolve to ChosenFiles: { id, path, name, kind }.
const files = await dialog.open({
  title: "Choose images",
  multiple: true,
  filters: [{ name: "Images", extensions: ["png", "jpg", "webp"] }],
});
const dir = await dialog.open({ directory: true });        // ChosenFile[], kind "folder"
const target = await dialog.save({ defaultName: "export.json" }); // ChosenFile | null

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
`multiple: false` (the default) and read `[0]`. `dialog.save` returns the
file or `null`.

## A file the user chose

What a dialog returns, and what [`onFileDrop`](windows.md) hands over, is a
`ChosenFile`: `{ id, path, name, kind }`. Pass it to a command as it is, and
take it as `elyra::files::ChosenFile`:

```rust
use elyra::files::ChosenFile;

#[command]
async fn import_notes(ctx: Ctx, file: ChosenFile) -> elyra::Result<usize> {
    let text = std::fs::read_to_string(file.path())
        .map_err(|e| elyra::Error::Command(format!("{}: {e}", file.name())))?;
    // …
}
```

```ts
const [file] = await dialog.open({ filters: [{ name: "Markdown", extensions: ["md"] }] });
if (file) await commands.importNotes({ file });
```

The shell grants each file the user picks or drops for the rest of the run,
and the command's argument is decoded from that grant's `id` alone. So the
frontend can't make a command read a path the user didn't choose: a made-up
`id` or a bare path doesn't decode, and a `path` the frontend changed is
ignored. That holds even if a script in the page is compromised.

`kind` says how it was chosen, which is what the file is for: `open` (picked
or dropped, to read), `save` (picked in a save dialog, to write) or `folder`
(a new file may be written in it). Rust code may grant its own files with
`ChosenFile::from_path(..)`, `ChosenFile::for_saving(..)` and
`ChosenFile::folder(..)`, which is also how a test stands in for the user.
A `ChosenFile` works wherever a path does (`AsRef<Path>`), such as
`Http::attach(..)`.

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

In an app, the frontend's `notify(..)` goes through the container's
`Notifier`, so a test swaps in `Notifier::fake()` and asserts on what was
shown instead of filling the machine's notification center. Laravel's
notifications use it too ([backend](backend.md#laravels-notifications)).

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
