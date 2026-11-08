# RFC 0030 — The app's identity on Windows

**Status:** implemented (2026-11-08) · **Target:** 0.44.0 · follows [RFC 0029](0029-installers.md) and [RFC 0008](0008-laravel-notifications.md) · see [Decisions](#decisions)

## Summary

On Windows, every notification an Elyra app showed was shown as
**Windows PowerShell**, with PowerShell's icon, in the toast and in the
notification centre. A desktop app's notification goes out under an
AppUserModelID (AUMID), and `notify-rust` falls back to PowerShell's when
it's given none. The app now has its own, and says who it is:

```rust
App::new()
    .identifier("com.example.notes") // as [bundle] identifier
    .icon(Icon::png(include_bytes!("../icons/app.png")))
    .about(AboutInfo::new("Notes", env!("CARGO_PKG_VERSION")))
```

The notification shows *Notes* and its icon, and the notification centre
groups the app's notifications under it.

## Design

### The id

`App::identifier(..)`, the same reverse-DNS id as `[bundle] identifier` in
`elyra.toml` (as `App::deep_link` pairs with `[bundle] deep_link`). Without
it, one made of the app's name: `Elyra Example` → `elyra-example`. Windows
takes up to 128 characters and no spaces; an identifier it wouldn't take
is a [configuration problem](0025-commands-and-configuration.md), found
before anything starts.

### Saying who it is

Windows reads a desktop app's name and icon for an AUMID from
`HKCU\Software\Classes\AppUserModelId\<id>`: `DisplayName`, and `IconUri`,
an image file. The app writes them at each start, where it registers its
URL scheme: its name (`AboutInfo`), and its `App::icon` as a PNG beside its
settings (`%APPDATA%\<app>\notification-icon.png`), written again only when
it changed. No installer is needed, so a `cargo run` build's notifications
are the app's too. Every notification the `Notifier` shows (the frontend's
`notify`, Laravel's notifications) goes out under the id.

### The uninstaller

What the app wrote, the installer's uninstaller removes. `rata bundle` asks
the built app who it is, as it already asks whether it stays in the tray
(`ELYRA_BUNDLE_INSPECT` answers `"app_id"`), and the uninstaller deletes
that key. The PNG is under the app's data, which an uninstall leaves.

## Decisions

1. **The registry, not the shortcut.** An AUMID can also live on the Start
   menu shortcut (`System.AppUserModel.ID`), which NSIS writes only with a
   plugin, and then works only for an installed app. The registry key needs
   neither, and is how Microsoft documents a notification's name and icon
   for an unpackaged app.
2. **No explicit AUMID for the process.**
   `SetCurrentProcessExplicitAppUserModelID` would also change how the
   taskbar groups the app's windows: a pin made from the Start menu
   shortcut, which has no AUMID, would then be a second button beside the
   running app's. The taskbar keeps grouping by the executable; the AUMID
   names the notifications.
3. **The app writes it, not only the installer.** A portable folder and a
   development build show notifications too, and the app knows its own
   name and icon at run time.
4. **An id made of the name when none is given.** Names are already what
   the single-instance lock and launch at login are keyed by; an app that
   sets nothing still gets its own notifications.

## Not in this RFC

- Clicking a notification after the app has quit (COM activation, which
  needs a registered activator).
- The taskbar's jump list and an explicit process AUMID.

## Testing

- Unit tests: the id from `App::identifier` or the name, what Windows
  takes, and the icon's PNG read back as it was.
- `App::problems` with an identifier Windows wouldn't take; the inspect
  answer's `app_id`; the uninstaller's script removes the key; `rata
  bundle` reads `app_id` from the answer.
- CI on Windows: a notification shown through the `Notifier` under an id,
  found in Windows' own history of that id's notifications
  (`ToastNotificationManager.History`); the example app, started, has
  written its name and a PNG icon under its id; the installed app answers
  `com.elyra.example`, and the uninstaller removes the key.
