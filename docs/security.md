# Security

Elyra apps are a **webview with native capabilities**. Anything that executes in
the page — your code, a dependency, injected content — can reach the IPC surface.
This page describes what that surface is and the three mechanisms that gate it.

## The IPC surface

Everything lives under `elyra://localhost`:

| Route | What it does | Capability |
|---|---|---|
| `/__cmd/<name>` | invoke a `#[command]` | `Commands` |
| `/__events` | long-poll the event bus | always available |
| `/__about`, `/__cancel`, `/__deeplink/initial` | metadata, cancel, launch URL | always available |
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
`UpdaterInstall`. `UpdaterInstall`, `Updater` and `Sidecar` are additionally
rate-limited per window.

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
