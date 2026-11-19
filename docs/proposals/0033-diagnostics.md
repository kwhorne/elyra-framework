# RFC 0033 — Diagnostics the user can send

**Status:** accepted (2026-11-19) · **Target:** 0.48.0 · builds on [RFC 0014](0014-elyra-inspector.md) and [RFC 0026](0026-the-running-app-for-coding-agents.md) · see [Decisions](#decisions)

## Summary

When something goes wrong in a shipped app, the developer asks the user
"which version, which system, what did the log say?", and the user can't
answer. This gives every Elyra app one file the user can save, read and
attach to an email or a ticket: what the app is, where it runs, how it's
connected, and what happened just before.

```rust
App::new()
    .diagnostics()                                  // Help → Save Diagnostics…
```

```js
import { diagnostics } from "@elyra/runtime";

await diagnostics.save();      // the save dialog; the file's path, or null
const text = await diagnostics.text();   // the same, for a "Report a problem" form
```

The file is plain text the user can open and read before they send it.
Nothing leaves the machine unless the user sends it.

## Motivation

- **A released build is a black box.** `rata mcp dev` (RFC 0026) and the
  inspector (RFC 0014) see a debug build on the developer's machine. On a
  user's machine there's a release build and, unless the app set up
  `LogProvider::to_app_dir`, which `rata new` doesn't, no log on disk.
- **What's asked for is the same every time:** the app's version and
  Elyra's, the OS and its version, the webview's version, whether the app
  is online and signed in, which server, whether writes are waiting in the
  outbox, and the last errors.
- **What's sent must be safe to send.** A log copied by hand carries
  tokens, URLs with signatures and the user's home folder. The bundle is
  built to leave those out, and the user can see that it did.

## Current state (0.47)

- `elyra::log` writes to stderr, and to a rotating file only with
  `LogProvider::to_path` / `to_app_dir`. A panic is logged at error level.
- `Secret<T>` redacts its `Debug`. Nothing redacts a log line.
- The about box (`AboutInfo`) has the app's name and version.
- The outbox knows its counts (`Outbox::counts()`), the backend its
  servers and whether there's a token, and the connection whether it's
  online. Nothing puts them in one place.
- The page's own errors (an exception in a component, a rejected promise)
  go to the webview's console, which a user of a release build can't open.

## Design

### What's in it

One UTF-8 text file, `<app-slug>-diagnostics-<yyyy-mm-dd-hhmm>.txt`, with
sections in this order:

```text
Elyra diagnostics — Acme Notes 2.3.1
Saved 2026-11-19 14:02 +01:00

## App
name: Acme Notes · version: 2.3.1 · identifier: com.acme.notes
elyra: 0.48.0 · build: release · features: backend, database, tray, updater

## System
os: macOS 16.1 (25B78) · arch: aarch64 · locale: nb-NO
webview: WKWebView 621.1 · screens: 2 (1.0x, 2.0x) · dark mode: on
uptime: 3 h 12 min · memory: 212 MB

## Connection
online: yes · server: https://api.acme.example (1 of 2) · signed in: yes
live channel: connected · last sync: 14:01:55

## Outbox
waiting: 3 (customers: 2, invoices: 1) · refused: 1
refused: invoices #881 update — 422 "The due date must be after the issue date."

## Updates
channel: stable · checked: 13:40 · newest: 2.3.1 (this one)

## Windows
main (shown, /customers/7) · settings (hidden)

## The page
14:01:58 Error  TypeError: Cannot read properties of undefined (reading 'id')
                at CustomerCard.svelte:41
14:01:58 Error  an answer from /api/customers/7 was not JSON

## Log (the last 300 lines)
14:00:02 INFO  elyra::backend  signed in
14:01:58 WARN  elyra::backend  GET /api/customers/7: 500
…

## From the app
(sections the app added)
```

- **The log is always there.** The shell keeps the last 1,000 records in
  memory, whatever the sinks, so an app without a log file still has one
  to send. With a file sink, the bundle reads from the file and can reach
  back further (the last 300 lines by default).
- **The page's errors are there.** `@elyra/runtime` keeps the last 50
  uncaught errors and unhandled rejections (message, the first stack
  frame, when) and hands them to the shell when the bundle is made. In a
  release build, that's the only place they can be seen. Each is also
  written to the log at error level, with the target `page`, so a log
  file and `rata mcp dev`'s `last_error` have them too.
- **An app adds its own sections:**

  ```rust
  App::new().diagnostics_with(|d| {
      d.section("Sync", format!("last full sync: {}", store.last_full_sync()));
  })
  ```

### What's never in it

- **Tokens, keys and secrets.** Nothing from the keychain, the bearer
  token or `Secret<T>`. The log lines are redacted on the way in:
  `Bearer …`, `Authorization: …`, `token=`, `signature=`, `password=`,
  and the query string of a signed URL become `•••`.
- **The user's data.** No records, no outbox payloads (only the resource,
  the id, the operation and the server's message), no clipboard, no
  files, and no picture of the window.
- **The user's home folder.** Paths under it start with `~`.
- **Who the user is**, unless the app adds it in its own section: the
  bundle says "signed in: yes", not as whom.

### Where the user finds it

- **`App::diagnostics()`** adds **Help → Save Diagnostics…** to the app
  menu (on macOS, and in the window menu on Windows and Linux). It opens
  the save dialog and, once saved, shows the file in Finder or Explorer.
- **`diagnostics.save()`** and **`diagnostics.text()`** in
  `@elyra/runtime`, for an app's own "Report a problem" screen. `text()`
  returns what the file would hold, so a form can show it and send it to
  the app's server.
- **The tray menu**, for an app that lives there:
  `TrayConfig::diagnostics()`.

Without `App::diagnostics()`, the routes answer that diagnostics are off,
and the runtime's calls throw. An app that doesn't want the feature, or
has its own, doesn't get a menu item it didn't ask for.

### Sending it

This RFC stops at a file the user sends themselves. A **`Route::elyraDiagnostics()`**
in the Laravel package, which takes the text from a signed-in user and
stores it for the team, is the obvious next step. It's listed under
[Not in this RFC](#not-in-this-rfc) so the format can settle first.

### Security

- **Nothing is sent by the framework.** The bundle is written where the
  user saves it, or handed to the page that asked. Sending it is the
  user's act, or the app's own code.
- **Only the app's own windows can ask.** The routes are the shell's,
  behind the same origin and capability checks as every other one
  (`diagnostics` is a capability, granted by `App::diagnostics()`).
- **Redaction is tested against the log the framework writes**, not only
  against made-up lines: a test signs in, uploads, gets a signed URL and
  a 422, then checks the bundle for the token, the signature and the
  password.
- **The user can read it.** It's plain text, not an archive, so what's
  sent is what they saw.

## Implementation plan

1. `elyra::log`: an in-memory ring of the last 1,000 records, always on;
   the redaction, applied when a record is written to any sink.
2. `framework/src/diagnostics.rs`: the sections, built from what the
   container has (`Backend`, `Outbox`, `Updater`, the windows), the
   system's facts (OS, arch, locale, the webview's version from wry), and
   the app's own sections.
3. The routes `/__diagnostics/text` and `/__diagnostics/save`, the
   capability, `App::diagnostics()` / `diagnostics_with(..)`, the menu
   item and `TrayConfig::diagnostics()`.
4. `@elyra/runtime`: the error buffer (`window.onerror`,
   `unhandledrejection`), each error sent to the log as it happens,
   `diagnostics.text()` and `diagnostics.save()`.
5. Docs (`docs/diagnostics.md`, the security page), the `elyra` skill and
   guidelines for `rata ai:install`, and the example app.

## Testing

- Rust: each section from a container with and without the parts
  (no backend, offline, an outbox with refused writes); the ring keeps
  the last 1,000; the redaction on the patterns above and on the
  framework's own log lines; `~` for the home folder; the capability
  refused without `App::diagnostics()`.
- An end-to-end test against the fake backend: sign in, fail a write,
  make the bundle, and find the 422 in it and the token nowhere.
- `@elyra/runtime` (Vitest): errors and rejections kept, at most 50, and
  handed over; the calls' errors without the capability.
- CI's three platforms: the bundle is made in the bundled app's
  `ELYRA_BUNDLE_INSPECT` run, and its System section names the OS.

## Not in this RFC

- Sending it to the app's server (`Route::elyraDiagnostics()` in the
  Laravel package) or to a service such as Sentry.
- Crash reports: a native crash (not a panic) that ends the process before
  anything can be written.
- A picture of the window, or the page's DOM.
- Collecting it without the user (telemetry).

## Alternatives considered

- **A zip with the log files, the config and a screenshot.** More for the
  developer, but the user can't see what's in it, and a screenshot is the
  user's data. Plain text the user can read keeps the trust that makes
  them send it.
- **Telling users where the log file is.** Only works when the app has a
  file sink, finds no page errors, and sends the tokens with it.
- **Built into every app, without `App::diagnostics()`.** Simpler, but it
  adds a menu item and a capability an app didn't choose.

## Decisions

1. **Opt-in, with `App::diagnostics()`.** An app gets no menu item and no
   capability it didn't ask for; one with its own way of reporting keeps
   it.
2. **Plain text.** The user can open the file and read what they're about
   to send, and that's what makes them willing to send it. A server that
   wants fields can parse the sections' `key: value` lines.
3. **The in-memory log is always on.** The last 1,000 records, whatever
   the sinks and with or without `App::diagnostics()`: it costs a few
   hundred KB, and the log is there for whatever asks for it next.
4. **"Save Diagnostics…".** It says what happens: a file is saved.
   "Report a Problem…" would promise that something is sent.
5. **Who the user is stays out**, unless the app adds it in its own
   section. "Signed in: yes" is enough to tell most problems apart, and
   the user decides whether to say who they are.
6. **The page's errors are kept in a debug build too**, in the same
   buffer, and `rata mcp dev`'s `last_error` shows them with the log's:
   an agent working on the app sees the exception a component threw.
