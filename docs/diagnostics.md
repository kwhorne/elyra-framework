# Diagnostics the user can send

When something goes wrong in a shipped app, the developer asks "which
version, which system, what did the log say?", and the user can't answer.
`App::diagnostics()` ([RFC 0033](proposals/0033-diagnostics.md)) gives the
user one plain-text file to save, read and attach to an email or a ticket.

```toml
# Cargo.toml: the save dialog is the `system` feature's
elyra = { version = "0.48", features = ["system"] }
```

```rust
App::new()
    .diagnostics()                                  // Help → Save Diagnostics…
```

```js
import { diagnostics } from "@elyra/runtime";

await diagnostics.save();                // the save dialog; the file's path, or null
const text = await diagnostics.text();   // the same, for a "Report a problem" form
```

Nothing leaves the machine unless the user sends it.

## What's in it

`acme-notes-diagnostics-2026-11-19-1402.txt`, in this order:

```text
Elyra diagnostics — Acme Notes 2.3.1
Saved 2026-11-19 14:02 +01:00

## App
name: Acme Notes · version: 2.3.1 · identifier: com.acme.notes
elyra: 0.48.0 · build: release · features: backend, database, system, updater

## System
os: macOS 16.1 (25B78) · arch: aarch64 · locale: nb-NO
webview: WKWebView 621.1 · screens: 2 (1x, 2x) · dark mode: on
uptime: 3 h 12 min

## Connection
online: yes · server: https://api.acme.example (1 of 2) · signed in: yes
live channel: connected

## Outbox
waiting: 3 (customers: 2, invoices: 1) · refused: 1
refused: PATCH /api/invoices/881 — validation: "due: The due date must be after the issue date."

## Updates
from: the backend (/api/desktop/update) · checked: 13:40 · up to date

## Windows
main (shown, focused, /customers/7) · settings (hidden)

## The page
14:01:58 Error     [main] TypeError: Cannot read properties of undefined (reading 'id')
                   at CustomerCard.svelte:41

## Log (the last 300 lines)
11-19 14:00:02 INFO  [elyra::auth] signed in to https://api.acme.example
11-19 14:01:58 WARN  [elyra::http] GET api.acme.example/api/customers/7 → 500 in 41ms
…

## From the app

## Sync
last full sync: 14:01
```

- **The log is always there.** The last 1,000 records are kept in memory,
  whatever the sinks, so an app without a log file still has one to send.
  With a [file sink](logging.md), the file reaches back further. Times are
  the user's own.
- **The page's errors are there.** `@elyra/runtime` sends each uncaught
  error and unhandled rejection to the shell, which keeps the last 50 and
  writes each to the log at error level with the target `page`. In a
  release build, where the webview's console can't be opened, that's the
  only place they're seen. It's done in every app, with or without
  `App::diagnostics()`, so a log file and `rata mcp dev`'s `last_error`
  have them too.
- **A section that has nothing** says so: `no backend`, `no outbox`,
  `no updater`.

### The app's own sections

```rust
App::new().diagnostics_with(|d| {
    d.section("Sync", format!("last full sync: {}", store.last_full_sync()));
})
```

They come last, under `## From the app`, and are made each time the file
is. Their text is redacted like the log's and the home folder becomes `~`,
but keeping the user's data out of them is the app's job.

## What's never in it

- **Tokens, keys and secrets.** A log record is redacted before it reaches
  any sink: a bearer token, an `Authorization` header, the value of
  `token=`, `password=`, `secret`, `api_key` and their kin (`access_token`,
  `"password": "…"`), and the whole query string of a URL that carries a
  `signature` become `•••`. `elyra::log::redact` is that function, for an
  app that writes elsewhere.
- **The user's data.** No records, no outbox payloads (only the method, the
  path and the server's answer), nothing from the clipboard or the window,
  and a page's path without its query.
- **The user's home folder.** A path under it starts with `~`.
- **Who the user is.** "signed in: yes", not as whom. An app that wants it
  in the file adds it in its own section.

## Where the user finds it

- **Help → Save Diagnostics…** in the app menu: on macOS in the Help menu,
  on Windows and Linux in the window's menu bar. It opens the save dialog
  in Downloads and, once saved, shows the file in Finder or Explorer.
- **The tray**, for an app that lives there:
  `TrayConfig::new().diagnostics()` puts the item in the tray's menu,
  where it's called.
- **The page**: `diagnostics.save()` and `diagnostics.text()`. `text()`
  returns what the file would hold, so a form can show it and send it to
  the app's server.
- **Rust**: `elyra::diagnostics::text(&ctx).await`, `None` without
  `App::diagnostics()`.

## Off unless asked for

Without `App::diagnostics()` there's no menu item, `Capability::Diagnostics`
isn't granted, and `/__diagnostics/*` is refused: `diagnostics.text()` and
`.save()` throw. A window can be denied it with
`WindowConfig::deny(Capability::Diagnostics)`.

These are [configuration problems](configuration.md), and the app doesn't
start:

- `App::diagnostics()` without elyra's `system` feature (the save dialog).
- `TrayConfig::diagnostics()` without `App::diagnostics()`.

## Related

- [Logging](logging.md) · [Security](security.md) · [Inspector](inspector.md)
  · [AI agents](ai-agents.md) (`rata mcp dev`)
