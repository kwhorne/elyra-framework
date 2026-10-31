# Security

Elyra apps are a **webview with native capabilities**. Anything that executes in
the page — your code, a dependency, injected content — can reach the IPC surface.
This page describes what that surface is and the four mechanisms that gate it.

## The IPC surface

Everything lives under `elyra://localhost`:

| Route | What it does | Capability |
|---|---|---|
| `/__cmd/<name>` | invoke a `#[command]` | `Commands` (+ the command's own ability) |
| `/__events` | long-poll the event bus | always available |
| `/__about`, `/__cancel`, `/__deeplink/initial` | metadata, cancel, launch URL | always available |
| `/__i18n`, `/__i18n/locale` | the translation catalog, switching locale | always available |
| `/__window/*` | minimize / close / resize / title | `Window` |
| `/__store/*` | read + write `settings.json` | `Store`, `StoreClear` |
| `/__cache/*` | the in-process cache | `Cache`, `CacheFlush` |
| `/__storage/*` | files inside the storage disk | `StorageRead`, `StorageWrite`, `StorageDelete` |
| `/__queue/push` | enqueue a background job | `Queue` |
| `/__sidecar/*` | spawn / write / kill child processes | `Sidecar` (+ program allowlist) |
| `/__sys/*` | dialogs, clipboard, notifications, `shell.open`, paths | `System` |
| `/__autostart/*` | launch at login | `Autostart` |
| `/__update/*` | check / install an update | `Updater`, `UpdaterInstall` |

## 1. The IPC token

Every `/__*` request must carry `x-elyra-token`. The shell generates a random
256-bit token per launch and injects it into the webview *before any page script
runs* (`globalThis.__ELYRA__.token`); `@elyra/runtime` attaches it automatically.
A document the app never loaded — a remote iframe, a page fetched cross-origin —
doesn't have it and gets `403` (`ForbiddenError` on the frontend).

## 2. Origin isolation

A production build sends **no** `Access-Control-Allow-*` headers, so a foreign
origin can't read an IPC response. CORS is only added under `rata dev`, and only
for the exact `ELYRA_DEV_URL` origin.

## 3. Capabilities

Everyday routes are granted by default; destructive ones are opt-in:

```rust
use elyra::security::Capability;

App::new()
    .allow_frontend(Capability::UpdaterInstall) // enable the update toast's install button
    .allow_frontend(Capability::StorageDelete)
    .deny_frontend(Capability::Store)           // keep settings Rust-only
```

Opt-in by default-deny: `StoreClear`, `CacheFlush`, `StorageDelete`,
`UpdaterInstall`. `UpdaterInstall`, `Updater`, `Sidecar` and `Assistant` are
additionally rate-limited per window.

## 4. Per-command abilities

`Capability::Commands` is a single grant covering all of `/__cmd/*`, so one XSS
reaches *every* command an app registers. An ability takes a command back out of
that blanket grant — Laravel's `Gate`/`Policy` idea, resolved at the IPC edge:

```rust
#[command(can = "posts.delete")]
async fn delete_post(ctx: Ctx, id: i64) -> elyra::Result<()> { /* … */ }
```

```rust
App::new()
    .allow_ability("posts.delete")              // one ability
    .allow_abilities(["posts.create", "posts.update"])
    .allow_ability("billing.*")                 // a whole namespace
```

- **Deny by default.** Until the app grants it, `delete_post` answers `403` with
  `x-elyra-error-kind: forbidden` and names the missing ability.
- **Opt-in per command.** A command with no `can = ..` is unchanged: the blanket
  `Commands` grant is enough, so existing apps need no edits.
- **The webview only.** Rust-side dispatch — a provider, a queue job, `TestApp` —
  is never gated by an ability, the same way `Sidecar::spawn` stays open while
  frontend spawn is allowlisted.
- **Grants may end in `*`** to cover a namespace; a bare `"*"` grants everything,
  which turns the declarations into documentation rather than a boundary.

With a Laravel backend and `Backend::abilities()` ([RFC 0010](proposals/0010-laravel-abilities.md)),
the signed-in user must have the ability too, by Laravel's list: the app's
grant says what its webview may reach at all, the user's list narrows it.
A command the user lacks answers kind `denied` (as Laravel's `403` does),
before any request, and isn't queued offline. Signed out, the user has none.
Laravel still authorizes every request; this keeps the desktop from
offering, or queueing, what it would refuse.

Declaring an ability is worth it for anything a hostile script would want and a
normal session rarely needs: deletion, billing, key rotation, anything that
spends money or leaves the machine.

## Content-Security-Policy

[`DEFAULT_CSP`](https://docs.rs/elyra/latest/elyra/security/constant.DEFAULT_CSP.html)
is served with every HTML response: `default-src 'self' elyra:`, no `object-src`,
no framing. Widen it with `App::csp(..)`; `App::csp_disabled()` turns it off (only
while debugging a policy problem).

## Dangerous operations

* **`sidecar.spawn` is deny-by-default.** Name the programs your app ships with:
  `App::sidecar_allow("ffmpeg")`. Rust-side `Sidecar::spawn` is unrestricted.
* **`shell.openExternal` is scheme-gated.** `http`, `https`, `mailto` (widen with
  `App::allow_open_schemes`); `file:` URLs, relative or missing paths, and
  anything executable are refused. `App::deny_open_paths()` blocks paths entirely.
* **Request bodies are bounded.** 16 MiB by default (`App::max_request_body`), and
  MessagePack nesting deeper than 64 levels is refused before it reaches serde.
* **Files come from the user, not the page.** A command that reads or writes a
  file the frontend names should take an `elyra::files::ChosenFile`, not a
  `String`. It decodes only from a grant the shell made when the user picked
  the file in a dialog or dropped it, so a compromised page can't make the app
  read `~/.ssh` or overwrite a file the user didn't choose
  ([a file the user chose](system.md#a-file-the-user-chose)).

## AI agents (MCP)

An app that calls `App::mcp(..)` lets an AI agent call the commands whose
abilities it grants ([RFC 0003](proposals/0003-mcp-server.md)) — a grant
separate from the frontend's, and empty by default.

* **No network listener.** The agent's client launches `myapp --mcp`, which
  talks MCP on stdio. While the app runs, that shim reaches it over a private
  local endpoint: a `0600` Unix socket, or on Windows a loopback port.
* **The endpoint checks a per-install token without sending it.** Both sides
  prove they hold it with an HMAC over fresh nonces, the server first. A
  process that can't read the token file can't call a tool, and one that
  takes the socket path or a port first never learns the token or sees the
  agent's calls — the shim then runs the app headless instead.
* **What can't be undone waits for the user.** A tool whose ability
  `Mcp::confirm` names runs only after the user accepts in the MCP client
  (elicitation); a client that can't ask gets an error, never a silent run.
  The round trip's `requestState` is HMAC-signed, bound to the tool, the
  arguments and the client, short-lived, and good for one run.
* **Calls go through the middleware**, with `CommandRequest::origin` set to
  `Origin::Agent { client }`.
* **Rate limited and visible.** Each tool takes 60 calls a minute by default
  (`Mcp::rate_limit`), and every call is logged, dispatched as `AgentCalled`
  and shown on the `elyra:mcp` channel — see [AI agents](mcp.md).

## The app's assistant

An app that calls `App::assistant(..)` gives each window an assistant whose
tools are the commands the assistant's grant covers
([RFC 0011](proposals/0011-in-app-assistant.md)). The window reaches it on
`/__assistant/*` (`Capability::Assistant`, 60 calls a minute).

* **The user's rights bound it.** With `Backend::abilities()`, a tool is
  offered only when the signed-in user has its ability, and checked again
  when it's called, so a sign-out or a role change mid-turn stops the next
  call. Signed out, it has no tools. Unlike an MCP agent, it never has more
  than the user.
* **What can't be undone waits for the user, in the app.** A tool whose
  ability `Assistant::confirm` names sends a question to its window and runs
  only after `/__assistant/confirm` accepts it. The id is random, single-use,
  for that window only, and expires after five minutes. The model has no
  tool for it.
* **Prompt injection is expected.** A customer's notes can ask the model to
  delete everything. The call is still held to the user's abilities, to
  `confirm` and to the rate limit, so put everything destructive behind
  `confirm`. The system prompt tells the model that tool results are data.
* **Data leaves the machine.** Tool results go to the model provider. Keep
  commands whose data mustn't leave out of the grant. Each turn also sends
  a note of the window ([RFC 0013](proposals/0013-assistant-sees-the-window.md)):
  its route, and its live queries' commands and arguments, only for
  commands the user is offered, never their rows.
  `Assistant::window_context(false)` turns it off.
* **Calls go through the middleware** with `Origin::Assistant`, and are
  logged and dispatched as `AgentCalled` with `client: "assistant"`, without
  their arguments.

## A Laravel backend

With the `backend` feature ([guide](backend.md)), the user's Sanctum token
lives in the OS keychain only. The webview signs in and out through
`/__auth/*` (`Capability::Auth`) and never sees the token. `Http` refuses
plain HTTP except to loopback, follows no redirect away from HTTPS, and drops
`Authorization` on a redirect to another host. Its logs never carry a query,
a body or a header.

Broadcasting ([RFC 0006](proposals/0006-laravel-broadcasting.md)) is `wss`
only, except to loopback. The app subscribes only to the channels it follows,
and the backend authorizes each private one for the signed-in user, so the
Pusher secret never leaves the server. An event's payload is untrusted data:
re-running a live query doesn't read it, and the frontend gets it as JSON.

Working offline ([RFC 0007](proposals/0007-offline-and-sync.md)) keeps the
last answer to each read in the app's database, the same place as the app's
own data. It's per user, with the user and the URL stored as hashes, kept
only while signed in, and cleared when the user signs out or someone else
signs in. `.no_offline()` keeps a request out of it. An app whose data needs
encrypting at rest gives `App::database` an encrypted SQLite.

Writes marked `.queue_offline()` wait in the outbox, in the same database. A
kept write holds its method, path, body, its own headers and its
`Idempotency-Key`, but never the token: it's sent with the one in the
keychain at the time, as the same user. A repeated key lets the server answer
from its record instead of making the write twice.

Notifications ([RFC 0008](proposals/0008-laravel-notifications.md)) arrive
on the signed-in user's own private channel, with the `id` from `/api/user`,
and the server's channel rule decides who hears it. Their text shows in the
OS's notification center, and maybe on a locked screen:
`.notification_previews(false)` shows only "New notification". A click opens
only a path in the app, through its router, never a full URL, so a
notification's data can't open a page the app didn't choose. Signing out
clears the list, its copy and the badge. The page reaches the notifications
through `/__auth/notifications*` (`Capability::Auth`): it can read them and
mark them read, nothing else.

Files ([RFC 0009](proposals/0009-files.md)) go to and from the backend as the
user chose them: an upload sends a `ChosenFile` from a dialog or a drop, and
a download writes only where the user chose to save, or a new file in a
folder they chose or their Downloads folder. A server's file name is cleaned
before it's used (no folders, no leading dot, no device names) and never
replaces a file. Downloads are marked as from the internet, quarantine on
macOS and the Mark of the Web on Windows, and the framework never opens
them. A signed URL (`Backend::download_url`) is fetched without the token,
and a redirect to another host drops it as always. Logs never carry a
file's name.

## Secrets

Don't put tokens in `Store` (plain JSON on disk) — use
[`Secrets`](https://docs.rs/elyra/latest/elyra/secrets/index.html) (feature
`secrets`), which stores them in the OS keychain. Never ship an AI provider key
inside a distributed binary; proxy through your own backend.

## Updates

The updater verifies an ed25519 signature over the downloaded artifact against the
key compiled into your app, requires **HTTPS** for the manifest and the artifact
(loopback excepted), streams to disk with a size cap, and on macOS additionally
verifies the code signature and bundle identity before swapping the `.app`.
Keep your signing key offline; a compromised release server still can't ship a
malicious update, but it can withhold one.

## Release checklist

1. Only the capabilities you actually use are granted.
2. `sidecar_allow` lists exactly the programs you ship.
3. CSP is on (or consciously widened) and the app loads no remote code.
4. Tokens/credentials go through `Secrets`, not `Store`.
5. Command input is validated (`elyra::validation`) — the frontend is untrusted.
6. `rata bundle` output is signed/notarized by your release pipeline.

## Reporting

See [SECURITY.md](../SECURITY.md) for how to report a vulnerability.
