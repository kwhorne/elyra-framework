# RFC 0052 — Open documents: files the OS hands the app

**Status:** accepted (2026-11-24) · **Target:** 0.54.0 · builds on [single-instance & deep-linking](../single-instance.md), [a file the user chose](../system.md#a-file-the-user-chose), [`rata bundle`](../cli.md) · see [Decisions](#decisions)

## Summary

A document app has to open its documents the way the OS offers them:
double-clicked in Finder or Explorer, dragged onto the Dock icon, chosen in
"Open With", or passed on the command line. Elyra registers URL schemes
(`deep_link`), but no file types. The bundle doesn't declare any, so the OS
never offers the app, and a file that reaches it anyway isn't handled. On
macOS it currently arrives as a raw `file://` string on `elyra:deep-link`.

This RFC adds document types: declared once, registered by `rata bundle`
on each platform, and delivered to the page as `ChosenFile`s the user chose.

```toml
# elyra.toml
[[bundle.documents]]
name = "Kver document"
extensions = ["kver"]
mime = "application/vnd.kver.document+json"
role = "editor"                 # or "viewer"
```

```rust
App::new()
    .opens(["kver"])            // the extensions this app accepts from the OS
    .single_instance()          // so a second double-click reaches this process
    .run()?;
```

```ts
import { documents } from "@elyra/runtime";

for (const file of await documents.initial()) openTab(file);   // opened at launch
documents.onOpen((files) => files.forEach(openTab));           // opened while running

// A ChosenFile, as from a dialog: pass it to a command as it is.
await commands.readDocument({ file });
```

- **One declaration, three platforms**: `rata bundle` writes macOS's
  `CFBundleDocumentTypes` and an exported type, a `.desktop` `MimeType` and a
  shared-mime-info XML in the `.deb`, and the NSIS installer's ProgID.
- **Granted like a dialog's file**: what the OS opens is something the user
  chose, so it arrives as a `ChosenFile` the page can pass to a command, and
  a command can't be made to read a path the user didn't open.
- **Launch and while running alike**: files at launch wait until the page
  asks for them; later ones arrive on an event. On Windows and Linux, the
  second process forwards them through single-instance.

## Motivation

- **Dogfood.** Porting kver (a Tauri document editor) found this as the
  first missing piece. Its `.kver` files open from Finder through Tauri's
  `fileAssociations` and `RunEvent::Opened`.
- **Every document app needs it**: an editor, a viewer, a tool for one file
  format. The URL scheme `deep_link` covers links, not files.
- **What exists today is a leak, not a feature.** On macOS, `Event::Opened`
  forwards every URL, files included, to `elyra:deep-link` as a string. The
  path is not granted and not filtered by type, and the frontend has to
  tell a file from a link itself.

## Current state (0.53)

- `App::deep_link(scheme)` reads a launch URL from argv
  (`deeplink::url_in_args`). It registers the scheme at startup on Windows
  (HKCU) and Linux (`.desktop` + `xdg-mime`). On macOS `rata bundle` writes
  `CFBundleURLTypes`.
- `App::single_instance()`: a second process hands one payload of at most
  8 KB to the first and exits. The payload is the deep-link URL from argv,
  or "".
- `shell/webview.rs`, macOS only: `Event::Opened { urls }` emits each URL
  on `elyra:deep-link`.
- `ChosenFile { id, path, name, kind }` with `FileChoice::{Open, Save,
  Folder}`, granted for the run. It decodes in a command only from a grant's
  `id`.
- `rata bundle`: the `.app`'s `Info.plist`, a `.deb` with a `.desktop` entry
  (`Exec=<bin> %u`), an AppImage, and an NSIS installer with the scheme in
  HKCU.

## Design

### Declaring document types

`[[bundle.documents]]` in `elyra.toml`, any number of them:

| key | | |
|---|---|---|
| `name` | required | shown by the OS ("Kind: Kver document") |
| `extensions` | required | without the dot, lowercase |
| `mime` | optional | Linux's type and macOS's `UTTypeTagSpecification`; default `application/x-<app>-<first extension>` |
| `role` | `editor` (default) or `viewer` | macOS's `CFBundleTypeRole`; Linux and Windows have no equivalent |
| `icon` | optional | a document icon; without it, the OS's generic one |
| `uti` | optional | macOS's type identifier; default `<identifier>.<first extension>` |
| `owned` | `true` (default) | the app defines the type (exported); `false` for a type another app or the system defines (`md`, `txt`), imported and only offered as "Open With" |

`App::opens([...])` lists the extensions the running app accepts. The bundle
gets the OS to offer the app, and the app decides what it takes, so a file
with another extension (via "Open With…" → Other) is turned away. `rata
check` reports a declared extension the app doesn't open, and one it opens
that no document type declares.

### Registration (`rata bundle`)

- **macOS**: the `Info.plist` gets `CFBundleDocumentTypes`, one entry per
  type with `CFBundleTypeName`, `CFBundleTypeRole`, `LSItemContentTypes`
  `[<uti>]` and `LSHandlerRank` (`Owner` when owned, `Alternate`
  otherwise). Owned types also go in `UTExportedTypeDeclarations`
  (conforming to `public.data`, plus `public.content`, with the extensions
  and MIME type); the others go in `UTImportedTypeDeclarations`. An icon
  is converted to `.icns` like the app's own.
- **Linux (`.deb`)**: the `.desktop` entry gets `MimeType=<mime>;…`, and
  `Exec` takes `%F` (files) when there are document types, alongside the
  `%u` a deep link needs. Owned types get
  `/usr/share/mime/packages/<identifier>.xml` (shared-mime-info: the
  glob, comment and icon). `postinst` runs `update-mime-database` and
  `update-desktop-database`. **AppImage** can't register types without
  something like appimaged, and the docs say so.
- **Windows (NSIS)**: per type a ProgID `<identifier>.<ext>` with its name,
  icon and `shell\open\command` `"<exe>" "%1"`, and `.<ext>` →
  `OpenWithProgids`. Owned types also become the default; the others are
  only offered. All of it goes under HKCU, like the scheme, and the
  uninstaller removes it. `SHChangeNotify` runs after both.

`App::opens` registers nothing at runtime. File types belong to an install,
unlike the scheme that `deep_link` registers at startup today (open
question 4).

### Delivery

A file is accepted when it exists, is a regular file, and has an extension
the app `opens`. Then it is granted as a `ChosenFile` and handed over.

- **macOS**: `Event::Opened { urls }`: each `file://` URL is a candidate.
  Other URLs keep going to `elyra:deep-link` as before, and files stop
  going there.
- **Windows and Linux at launch**: argv. Each argument that isn't a flag or
  the deep link and names an accepted file is a candidate.
- **While running** (Windows, Linux): the second process forwards its
  candidates through single-instance. The payload becomes a small JSON
  object, `{ "url": …, "files": […] }`, still capped at 8 KB. A plain
  string from an older build is still read as the URL. The primary checks
  every forwarded path again before granting it.
- **Before the page is ready** (all three): accepted files queue up until
  `documents.initial()` takes them. After that, each batch goes out on
  `elyra:open-documents` to every window, and the app decides which window
  opens it.

### The grant

What the OS hands over is something the user chose, so it is granted the
way a dialog's file is. Its kind is `FileChoice::Document`, new in this RFC:
opened from the OS, to read and to save back to (open question 1).

### Frontend and Rust API

```ts
documents.initial(): Promise<ChosenFile[]>        // the launch files; each is handed out once
documents.onOpen(cb: (files: ChosenFile[]) => void): () => void
```

```rust
App::opens(extensions)                            // what the app accepts
App::on_open(|ctx: &Ctx, files: Vec<ChosenFile>| { … })   // optional: Rust sees them first
```

With `on_open`, Rust handles each batch (to show a window per document, or
to record recents) and the page still gets it.

### Testing

- **Offline (unit)**: generating `Info.plist`, `.desktop`, shared-mime XML
  and NSIS, each with no, one, and several types, owned and not; argv
  parsing (flags, the deep link, paths with spaces, other extensions,
  folders, missing files); the single-instance payload both ways, including
  an old plain string; the queue before `initial()` and the event after it;
  and the grant's kind.
- **CI on Linux**: the `.deb` installed in the bundle job, then
  `xdg-mime query filetype x.<ext>` gives the MIME type and `xdg-mime query
  default <mime>` gives the app's `.desktop`. The example app started with a
  file argument, and the page reads it from `documents.initial()`.
- **CI on Windows**: after the silent install, the ProgID and
  `OpenWithProgids` are in HKCU. `Start-Process x.<ext>` reaches the running
  app.
- **CI on macOS**: `lsregister -f` on the built `.app`, then `open x.<ext>`.
  The page gets it from `onOpen` (the app is running) and from `initial()`
  (it isn't). If LaunchServices proves flaky on the runner, this part stays
  checked by hand, and the docs say so.

## Implementation plan

1. `[[bundle.documents]]` in `ratatosk/src/config.rs`; the plist, `.desktop`,
   mime XML, `postinst` and NSIS output; `rata check`.
2. `App::opens` and `App::on_open`, `FileChoice::Document`, argv candidates,
   the JSON single-instance payload, macOS `Opened` split into files and
   URLs, the queue and `elyra:open-documents`.
3. `documents` in `@elyra/runtime`, codegen's types, and the example app
   opening a `.elyranote`.
4. Tests, CI steps, docs (`docs/documents.md`, configuration, single-instance,
   system), CHANGELOG, roadmap.

## Open questions

Answered in [Decisions](#decisions).

1. **The grant's kind.** A new `FileChoice::Document` (read and save back),
   or `Open` like a dropped file?
   *Recommendation:* `Document`. An editor saves back to the file it opened,
   and with `Open` the app would have to ask a save dialog for a path the
   user already chose, or ignore `kind`. Adding a variant to `FileChoice`
   breaks an exhaustive `match`, so it's called out under Upgrading (or
   `FileChoice` becomes `#[non_exhaustive]` in the same release).
2. **Does `opens` repeat the bundle?** The extensions could come from
   `elyra.toml` at build time instead.
   *Recommendation:* keep `App::opens`. The runtime doesn't read
   `elyra.toml` today, and a test or `cargo run` should behave like the
   install. `rata check` keeps the two in step.
3. **Which window gets a file?** Every window gets the event, or only the
   focused one?
   *Recommendation:* every window, and the app decides (a document app
   usually has one window that opens tabs, or `on_open` opens a window).
   One window choosing for all makes "a window per document" harder.
4. **Register at runtime too**, as `deep_link` does on Windows and Linux,
   for `cargo run` and an AppImage?
   *Recommendation:* no. An app run from `target/` that claims file types
   leaves registrations behind that point at a build folder. File types
   belong to the install. The AppImage limit goes in the docs.
5. **The macOS file URLs on `elyra:deep-link` today.** Stop sending files
   there at once, or keep them for a release?
   *Recommendation:* stop. They were never documented as files, and they
   arrive ungranted. An app that relied on it gets the change under
   Upgrading, and its files come from `documents` instead.

## Not in this RFC

- **"Open Recent"** in the Dock and File menus (`NSDocumentController`),
  and Windows jump lists. kver keeps its own recents.
- **The menu itself** (check items, enabled state): that's RFC 0053.
- **Quick Look and thumbnail extensions** for the app's own types.
- **Saving as a type** (a save dialog filtered by the declared types)
  beyond today's `dialog.save({ filters })`.
- **`.dmg`**, found by the same port: its own change to `rata bundle`.

## Alternatives considered

- **Keep using `deep_link`** with a `file` scheme: no type registration, an
  ungranted path, and no argv handling on Windows or Linux.
- **Rust-only delivery** (`on_open`, no frontend API): every app would
  write the same command and event to reach the page.
- **Plain paths to the frontend** instead of `ChosenFile`: simpler, but a
  page could then pass any path to a command, which `ChosenFile` exists to
  prevent.

## Decisions

1. **`FileChoice::Document`**: opened from the OS, to read and save back.
   It's called out under Upgrading (an exhaustive `match` needs the arm).
2. **`App::opens` stays**, and `rata check` keeps it in step with
   `[[bundle.documents]]`.
3. **Every window gets `elyra:open-documents`**, and the app decides.
4. **No runtime registration**: file types belong to the install, and the
   AppImage limit is documented.
5. **Files stop going to `elyra:deep-link` on macOS** at once, under
   Upgrading.
