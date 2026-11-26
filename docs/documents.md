# Opening documents

A document app opens its files the way the OS offers them: double-clicked
in Finder or Explorer, dropped on the Dock icon, chosen in "Open With", or
named on the command line. Elyra registers the app's document types when
it bundles the app, and hands the page each file the OS opens as a
[`ChosenFile`](system.md#a-file-the-user-chose)
([RFC 0052](proposals/0052-open-documents.md)).

```toml
# elyra.toml
[[bundle.documents]]
name = "Kver document"
extensions = ["kver"]
mime = "application/vnd.kver.document+json"
```

```rust
App::new()
    .opens(["kver"])        // what the running app takes
    .single_instance()      // so a second double-click reaches this process
    .command(read_document)
    .run()?;

#[command]
async fn read_document(_ctx: Ctx, file: ChosenFile) -> elyra::Result<String> {
    std::fs::read_to_string(file.path()).map_err(|e| elyra::Error::Command(e.to_string()))
}
```

```ts
import { documents } from "@elyra/runtime";

// Subscribe first, then take the launch's files, so none falls between.
documents.onOpen((files) => files.forEach(openTab));
for (const file of await documents.initial()) openTab(file);

async function openTab(file) {
  const text = await api.read_document(file);   // the ChosenFile, as it is
  // …
}
```

## Declaring the types

Each `[[bundle.documents]]` is one type, with:

| key | | |
|---|---|---|
| `name` | required | what the OS calls it ("Kind: Kver document") |
| `extensions` | required | lowercase, without the dot |
| `mime` | optional | default `application/x-<app>-<first extension>` |
| `role` | `editor` (default) or `viewer` | macOS's role for the app |
| `icon` | optional | the document's own icon, relative to the project; otherwise the OS's generic one. A path that isn't a file is an error |
| `uti` | optional | macOS's type identifier; default `<identifier>.<first extension>` for a type the app owns |
| `owned` | `true` (default) | the app defines the type and becomes its default app; `false` for a type defined elsewhere (`md`, `txt`), which is only offered in "Open With" |

`rata bundle` writes them for each platform:

- **macOS**: `CFBundleDocumentTypes`, with an exported type declaration for
  each type the app owns.
- **Linux (`.deb`)**: `MimeType` in the `.desktop` entry, a shared-mime-info
  file for the types it owns, the icons, and a `postinst` that refreshes the
  caches. An **AppImage** can't register types by itself.
- **Windows**: a ProgID per type in the installer, under `HKCU` like the
  rest, removed by the uninstaller. A type the app owns becomes the
  extension's default app.

The types are registered by installing the app, never at runtime: a build
run from `target/` doesn't claim them.

`App::opens([..])` says what the running app takes; a file of another type
that reaches it is turned away. [`rata check`](cli.md) reports an
extension `[[bundle.documents]]` registers that the app doesn't open, and
one it opens that nothing registers.

## What the page gets

Each file is a `ChosenFile` of kind `document`: one the user opened, which
a command may read and save back to. As with a dialog's file, a command
takes it by its grant alone, so the page can't make a command read a path
the user didn't open.

- **`documents.initial()`**: the files the app was launched with, handed
  out once, to the app's main window (its first): a second call gets `[]`,
  and so does any other window's, the tray's popover and a Quick Look
  included, without taking them. An app with no window of its own (only a
  tray and its popover) hands them to the page that asks.
- **`documents.onOpen(cb)`**: each batch opened while the app runs (on
  `elyra:open-documents`), sent to every window. The app decides which one
  opens it.
- **`App::on_open(|ctx, files| …)`**: the same batches in Rust first, to
  open a window per document or keep a list of recent ones. The page still
  gets them.

How they arrive: macOS's open-documents event; on Windows and Linux, the
command line, and a second launch's files forwarded through
[single-instance](single-instance.md). A Quick Look or other read-only
window can't take the launch's files.

## Related

- [A file the user chose](system.md#a-file-the-user-chose) · [Single-instance](single-instance.md) · [`rata bundle`](cli.md) · [Configuration](configuration.md)
