# RFC 0029 — Installers, and the `.exe`'s icon

**Status:** implemented (2026-11-08) · **Target:** 0.43.0 · builds on [RFC 0028](0028-tray-and-icons.md) and [RFC 0024](0024-backend-updates.md) · see [Decisions](#decisions)

## Summary

`rata bundle` gives a macOS app a proper `.app`, but on Windows a folder
with an `.exe` that has no icon of its own and shows in Task Manager by
its file name, and on Linux a `.deb` that doesn't say what it needs. An
app's users get an installer on Windows and one file on Linux:

```toml
[bundle]
name = "Notes"
version = "1.2.0"
icon = "app/public/icon.svg"
publisher = "Example AS"
deep_link = "notes"
```

```text
$ rata bundle            # on Windows
bundle: icon and version -> notes.exe
bundle: created target\release\bundle\Notes
bundle: created target\release\bundle\Notes_1.2.0_x64-setup.exe

$ rata bundle            # on Linux
bundle: created target/release/bundle/notes_1.2.0.deb
bundle: created target/release/bundle/notes_1.2.0.tar.gz
bundle: created target/release/bundle/notes_1.2.0_x86_64.AppImage
```

## Motivation

- **An app is known by its icon.** Explorer, the Start menu, the taskbar
  and Alt-Tab draw the `.exe`'s own icon. Without it the app looks
  unfinished before it has opened.
- **Users install apps.** A zip that unpacks to a folder somewhere isn't
  what Windows users expect: no Start menu entry, no uninstall, and a link
  (`notes://…`) works only after the first run.
- **Linux has more than Debian.** A `.deb` covers Debian and Ubuntu; an
  AppImage runs on most distributions, from one file.
- **The updater needs a place it can write.** An app installed under
  `Program Files` can't replace itself without an administrator.

## Design

### The `.exe`'s icon and version

`rata bundle` writes a compiled resource file (`.res`): the icon at 16 to
256 px as one icon group, and a version resource (`FileDescription` and
`ProductName` from `name`, the version, `CompanyName` from `publisher`).
Microsoft's linker takes a `.res` as it takes an object file, so the
binary is built with `cargo rustc --release --bin <app> -- -C
link-arg=<app>.res`: no `rc.exe`, no `build.rs` in the app, and the extra
argument reaches the binary alone, so nothing else rebuilds.

### Icons from one source

The icons are drawn by `rata` with `resvg` (an SVG drawn at each size; a
PNG scaled, by halves first). Small sizes are stored as bitmaps and 256 px
as a PNG, as Windows reads them. The same images make the installer's
`.ico`, the `.deb`'s `hicolor` PNGs and the AppImage's icon. macOS keeps
`sips` and `iconutil`.

### The Windows installer

An NSIS script, written by `rata` and compiled by `makensis` when it's
there. It installs **for the user who runs it** (`RequestExecutionLevel
user`), into `%LOCALAPPDATA%\Programs\<Name>`: no administrator's prompt,
and the updater can swap the executable. It adds the Start menu shortcut,
the URL scheme, and the Settings → Apps entry (`HKCU`, keyed by
`identifier`). Its uninstaller removes those and the launch-at-login entry
(`Run`, by the app's name), and leaves the user's data under `%APPDATA%`.

### The AppImage

An AppDir (the binary, `AppRun`, the `.desktop` entry, the icon), made into
an AppImage by `appimagetool` when it's on the `PATH` or in
`$APPIMAGETOOL`. It uses the system's WebKitGTK, as the `.deb` does, which
now says so: `Depends: libwebkit2gtk-4.1-0, libgtk-3-0`.

Inside an AppImage the executable is in a temporary mount. What starts the
app is the AppImage, named in `$APPIMAGE`, so launch at login, the deep
link's registration and the updater use it (`launcher()` in the
framework): an AppImage updates by replacing itself.

## Decisions

1. **NSIS, not WiX.** NSIS makes a per-user installer that needs no
   administrator, the case for apps that update themselves; `makensis` is
   one download, and runs on Linux and macOS too. An MSI is for
   organisations that deploy by Group Policy: next, if apps ask.
2. **The tools stay outside.** `makensis` and `appimagetool` aren't
   downloaded by `rata`. Without them, the portable folder or the AppDir
   is made and `rata bundle` says how to get the tool.
3. **Resources at link time, not in a `build.rs`.** The app's crate stays
   as it is; only `rata bundle`'s build carries the icon. A resource
   compiler would be one more tool on the machine, and the `.res` format
   is small and documented.
4. **The system's WebKitGTK.** Carrying WebKitGTK in the AppImage would
   make it some 100 MB and tie it to one build of the webview. A desktop
   that runs GNOME has it.
5. **Signing stays the app's.** Authenticode, like a Developer ID, needs
   the app's own certificate; it signs what `rata bundle` made.

## Not in this RFC

- An MSI, Flatpak, and a `.rpm`.
- Carrying WebKitGTK in the AppImage.
- The Windows notification identity (AppUserModelID) on the shortcut.

## Testing

- Unit tests on every platform: the icons drawn from an SVG and a PNG, the
  `.ico` and its bitmaps, the `.res` file and its version resource read
  back, the NSIS script, `AppRun`, the `.deb`'s directories, and
  `launcher()`.
- A `.res` linked by `lld-link` into a resource-only DLL reads back with
  `pefile`: the icons, their group and the version strings.
- CI, `rata bundle (…)` on each platform with the example app: on macOS the
  `.app`; on Linux the `.deb` installed with `apt` (its dependencies
  resolved) and started, and the AppImage started; on Windows the `.exe`'s
  icon and version read back, the installer run silently, the installed
  app started, and the uninstaller run, leaving nothing behind.
