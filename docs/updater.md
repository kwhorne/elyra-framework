# Auto-updater

Feature-gated behind `updater`. Two questions decide how an app updates
itself: **where it asks** whether there's a newer release, and **how it knows**
the download is genuine.

- **Where:** a fixed manifest URL ([`UpdaterConfig::new`]), or the signed-in
  user's own server ([`UpdaterConfig::from_backend`], [RFC 0024](proposals/0024-backend-updates.md)),
  for an app distributed from each customer's server behind sign-in.
- **How:** an **ed25519 signature** over the artifact, checked against a
  public key the app ships (Tauri's model), and/or, on macOS, a **Developer
  ID**: the artifact is a zip of the whole signed, notarized `.app`, and the
  bundle inside must be signed by the app's team over its bundle id,
  unaltered — the check Gatekeeper makes.

```toml
elyra = { version = "0.36", features = ["updater"] }            # a manifest URL
elyra = { version = "0.36", features = ["updater", "backend"] } # from the backend
```

## The built-in update component (recommended)

Most apps don't need the low-level API. Call [`App::updater`] with an
[`UpdaterConfig`] and the framework wires up the whole flow — a silent check,
the IPC endpoints, progress events, and a themed **update toast** in the
frontend (rendered by `@elyra/runtime`):

```rust
use elyra::{App, UpdaterConfig};

App::new()
    .title("MyApp")
    .updater(
        UpdaterConfig::new(
            PUBLIC_KEY_B64,
            "https://releases.example.com/latest.json",
            env!("CARGO_PKG_VERSION"),
        )
        .auto_check(true), // opt in to the silent startup check
    )
    .run()
```

What you get:

- **Startup check (opt-in)** — it is **off by default**; enable it with
  `.auto_check(true)`. When on, the shell checks the manifest silently on launch
  and, if a newer release exists, emits an `elyra:update` event so the toast
  appears: *↑ Update available: vX* with **What's new**, **Install & restart**,
  **Later**.
- **Install** — clicking *Install & restart* downloads + verifies the artifact,
  streaming *↓ Downloading… n%*, then puts it in place and relaunches.
- **Manual check** — from the frontend, `checkForUpdate()` returns an
  `UpdateCheck` and shows the toast if an update is available; wire it to a
  button or menu item. `installUpdate()`, `restartToUpdate()` and
  `dismissUpdate()` are also exported.

## Updates from the backend

An app that ships with a licence is downloaded from each customer's own
server, behind sign-in, not from a public URL. The server is chosen at
sign-in ([RFC 0021](proposals/0021-backend-chosen-at-sign-in.md)), so the
manifest can't be a URL fixed at build time:

```rust
App::new()
    .backend(Backend::chosen_at_sign_in().multiple_servers())
    .updater(
        UpdaterConfig::from_backend("/api/desktop/update", env!("CARGO_PKG_VERSION"))
            .developer_id("52AT44QFCM", "works.felagi.desktop") // macOS: who signs it, and what
            .auto_check(true),
    )
    .allow_frontend(Capability::UpdaterInstall)
```

**The request** goes to the active server, as the signed-in user, through the
app's [`Backend`](backend.md) (its token, its `401` handling, its server
switches):

```http
GET {server}/api/desktop/update?platform=macos-aarch64&version=1.4.0
Authorization: Bearer …
```

`platform` is `Updater::current_target()` (`{os}-{arch}`: `macos-aarch64`,
`macos-x86_64`, `windows-x86_64`, `linux-x86_64`), `version` the running one.

**The answer** is `204 No Content` when the app is up to date, or `200` with the
newest release for that platform (a `{ "data": … }` wrapper is fine):

```json
{
  "version": "1.5.0",
  "notes": "- Faster search\n- Fixes the sign-in on Windows",
  "released_at": "2026-11-06",
  "url": "https://acme.example.com/desktop/Felagi-1.5.0-macos-aarch64.zip",
  "size": 31457280,
  "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
}
```

- `version` (semver) and `sha256` (hex) are required; a `version` no newer
  than the running one is up to date, whatever the status. `notes`,
  `released_at` and `size` are optional; `signature` (base64 ed25519 over the
  artifact) is required when the app has a [`public_key`](#trust).
- **`url` must be on the active server's origin** — the same scheme, host and
  port. It's downloaded with the same token, so anything else is refused
  (`Error::ForeignOrigin`) and nothing is sent there. A path (`/desktop/…`) is
  resolved against the server. Serve the file from a route that checks the
  token, and redirect to object storage there if you must: a redirect to
  another host doesn't carry `Authorization`.
- **Signed out, nothing is asked.** A manual check answers
  `{ available: false, error: "not signed in…" }`.
- **Another server**: after `switch_to` or a sign-in elsewhere
  ([RFC 0022](proposals/0022-multiple-servers.md)), the next check asks the
  new active server, with its token. An offer from the old one is withdrawn
  (`up-to-date`) if the new one has nothing newer.

**The schedule** (with `auto_check`): 10 seconds after start, 3 seconds after
each sign-in, switch or sign-out, and every 24 hours. The same version isn't
offered twice in a row except by the daily check.

**Two steps.** Installing downloads, verifies and moves the new version into
place, then the toast says *✓ v1.5.0 is installed. Restart to use it.* with
**Restart** and **Later**. Nothing restarts until the user says so, so a chat
app isn't restarted mid-sentence; *Later* leaves the new version for the next
launch. `UpdaterConfig::relaunch_when_asked(bool)` chooses either way, for
either source (default: on for `from_backend`, off for `new`).

### On the Laravel side

[`elyracode/elyra-backend`](../laravel/README.md) serves both routes, inside
the `auth:sanctum` group:

```php
// routes/api.php
Route::middleware('auth:sanctum')->group(function () {
    Route::elyraUpdates();   // GET /api/desktop/update, GET /api/desktop/download/{file}
});
```

Each release is put on a disk by an artisan command, which records its size
and SHA-256 beside it:

```bash
php artisan elyra:release Felagi-1.5.0-macos-aarch64.zip 1.5.0 macos-aarch64 \
    --notes-file=CHANGES.md          # or --notes="…"; --signature=… for a public_key
```

- **Where:** `config('elyra.updates.disk')` (`ELYRA_UPDATES_DISK`, `local`
  by default) under `desktop-releases/`, the files beside a
  `releases.json`. Keep the disk private: the files are downloaded through
  the route, with the user's token.
- **What's offered:** the newest release for the platform the desktop
  names, when it's newer than the version it runs; otherwise `204`. The
  `url` is a path (`/api/desktop/download/<file>`), so it's on the origin
  the desktop asked, whatever `APP_URL` says.
- **A version again** for the same platform replaces the release. Two
  releases can't share a file name.
- **On macOS** the file is a zip of the signed `.app`, as the desktop
  installs it (`ditto -c -k --keepParent Felagi.app Felagi-1.5.0.zip`).

Who may update is who may sign in. For more (a licence, a beta channel),
write the route yourself; the answer is the one above:

```php
Route::get('/desktop/update', function (Request $request) {
    $release = DesktopRelease::latestFor($request->query('platform'));
    if (! $release || version_compare($release->version, $request->query('version'), '<=')) {
        return response()->noContent();
    }
    return [
        'version' => $release->version,
        'notes' => $release->notes,
        'url' => route('desktop.download', $release->file, false),   // a path: this server's origin
        'size' => $release->size,
        'sha256' => $release->sha256,
    ];
});
```

## Trust

Never installs bytes that weren't verified. What's checked:

| Configured | SHA-256 | ed25519 | Code signature (macOS bundle) |
|---|---|---|---|
| `new(key, url, v)` | when the manifest has one | **required** | signed and intact (`codesign --verify --strict --deep`), same bundle id as installed |
| `new(..).developer_id(team, id)` | when the manifest has one | **required** | **Developer ID** of `team` over `id` |
| `from_backend(..).developer_id(team, id)` | **required** | — | **Developer ID** of `team` over `id` |
| `from_backend(..).public_key(key)` | **required** | **required** | signed and intact, same bundle id as installed |
| `from_backend(..).developer_id(..).public_key(key)` | **required** | **required** | **Developer ID** of `team` over `id` |
| `from_backend(..)` alone | — | — | — refused: nothing would verify it, so the update flow is off, with an error in the log |

**A Developer ID** means all of:

1. `codesign --verify --strict --deep` passes: the bundle and everything
   nested in it is unaltered since it was signed.
2. `TeamIdentifier` is the configured team, and `Identifier` the configured
   bundle id.
3. It satisfies the code requirement Apple writes for a Developer ID app,
   narrowed to that team and id: anchored at Apple's root, a Developer ID
   intermediate, a Developer ID Application leaf issued to the team. A
   self-made certificate that merely names the team fails here.

For every bundle, it's the one `.app` at the top of the zip. From the backend
or with a Developer ID, its `CFBundleShortVersionString`, when it's semver,
must also be newer than the running version, so a server can't hand back an
older signed release.

The ed25519 key is still the right tool for a loose executable (Windows,
Linux) and for a manifest URL that anyone can fetch; the Developer ID needs
no separate key to keep and is what macOS checks anyway.

## Applying the update

| Where the app runs | What happens |
|---|---|
| macOS, an installed `.app` (`/Applications`, `~/Applications`, …) | The zip is expanded with `ditto -x -k` into a private directory (`0700`, under the user's temp dir) and checked as above. The installed bundle is **moved aside** (`.Foo.app.old-<pid>`, beside it), the new one **moved in** by a rename (a `ditto` copy across volumes), the quarantine attribute cleared from it, and the old one removed. If the new one can't move in, the old one is put back. A rename leaves the running executable's file untouched; overwriting it would make macOS kill the process. Leftovers of an interrupted swap are removed at the next start. |
| macOS, started from a build (`target/debug`, `target/release`, a `rata bundle` in `target/`) | Refused: *this app was started from a build…*, before anything is downloaded. |
| macOS, translocated (opened where it was downloaded) | Refused: *macOS is running this app from a temporary, read-only copy; move it to Applications…* |
| macOS, a folder the user can't write (another account's `/Applications`, a disk image) | Refused: *no permission to replace the app in …*; installing into `~/Applications` avoids it. |
| macOS, a loose executable | With a manifest URL and an ed25519 key, swapped in place as before; otherwise refused (*isn't running from a .app bundle*). |
| Windows, Linux | The artifact is the executable, swapped in place (the old one kept as `<exe>.old`). Needs an ed25519 key: a Developer ID alone is refused there (*a Developer ID only verifies a macOS app…*). |

**Relaunching** starts the new version once this process has exited — `open -n`
for a bundle, the executable otherwise — and quits the app as the tray's Quit
does (window geometry saved). Waiting for the exit matters to a
single-instance app: started first, the new copy would hand its launch to the
old one and quit. On Windows the new executable is started directly, as
before.

### Endpoints & events

| Path | Purpose | Capability |
|---|---|---|
| `GET /__update/check` | run a check, return `{ available, version?, notes?, error?, ready, restart? }` | `Updater` (default) |
| `POST /__update/install` | download + verify + put in place in the background | `UpdaterInstall` (opt-in) |
| `POST /__update/restart` | start the installed version and quit | `UpdaterInstall` (opt-in) |

Progress is pushed on the **`elyra:update`** channel as
`{ phase, version?, notes?, progress?, message?, restart? }`, where `phase` is
one of `available` / `downloading` / `ready` / `error` / `up-to-date`, and
`restart` is `"ask"` (two steps) or `"now"` (installing restarts).
`up-to-date` also withdraws an offer after a sign-out or a switch to a server
with nothing newer. `@elyra/runtime` subscribes to it automatically.

### The toast's words

English by default. An app replaces them with `setUpdateTexts`, or translates
them in its lang files as `elyra.update.<key>` (used once the catalog is
loaded, which any `$t` does):

```ts
import { setUpdateTexts, translate } from "@elyra/runtime";

setUpdateTexts({
  available: "Ny versjon: :version",
  install: "Installer",
  installed: ":version er installert. Start på nytt for å bruke den.",
  restart: "Start på nytt",
  later: "Senere",
});
```

The keys: `available`, `whatsNew`, `hideNotes`, `install`, `installAndRestart`,
`later`, `downloading` (`:percent`), `restarting`, `installed` (`:version`),
`restart`, `failed` (`:message`), `dismiss`.

## Checking for updates (low-level API)

```rust
use elyra::updater::{Updater, UpdateStatus};

let updater = Updater::new(PUBLIC_KEY_B64, env!("CARGO_PKG_VERSION"))?;
let target = Updater::current_target();          // e.g. "macos-aarch64"

match updater.check("https://releases.example.com/latest.json", &target)? {
    UpdateStatus::UpToDate => {}
    UpdateStatus::Available(info) => {
        let staged = updater.download_verified(&info)?; // signature-checked
        Updater::apply_and_relaunch(&staged)?;          // one step, never returns
    }
}
```

The built-in flow is [`UpdaterRuntime`] in the container:
`check()`, `check_for_frontend()`, `download(&info, progress)` (verified bytes,
not installed), `install()`, `relaunch()`, `install_target()`. The bundle steps
are [`elyra::updater::bundle`]: `installed_bundle(exe)`, `judge(signature,
developer_id)`, `install_bundle(..)`, behind the `BundleSystem` trait
(`NativeSystem` runs `ditto` and `codesign`), so a test can fake them.

## The manifest (a fixed URL)

```json
{
  "version": "1.2.0",
  "notes": "Bug fixes",
  "platforms": {
    "macos-aarch64": {
      "url": "https://releases.example.com/MyApp-1.2.0-macos-aarch64.zip",
      "signature": "<base64 ed25519 signature of the artifact>",
      "sha256": "<optional: hex SHA-256 of the artifact>"
    }
  }
}
```

On macOS the artifact is a zip of the whole `.app`
(`ditto -c -k --keepParent --norsrc MyApp.app MyApp.zip`); elsewhere, the
executable.

## API

- `Updater::new(public_key_b64, current_version)` — parse the key + version;
  `Updater::for_version(v)` without a key.
- `Updater::current_target()` — `"{os}-{arch}"`.
- `evaluate(manifest_json, target)` — pure: parse + semver compare, pick the
  target platform. Returns `UpdateStatus`.
- `verify(data, signature_b64)` — ed25519 verification with the bundled key.
- `check(manifest_url, target)` — HTTPS fetch + `evaluate`.
- `download_verified(info)` / `download_verified_with_progress(info, cb)` — stream
  the artifact to a private temp file (64 KiB chunks, `0600`, `O_EXCL`), verify its
  signature (and SHA-256, when given), and only then return the path. Never
  returns unverified bytes; a failed check removes the partial file.
- `verify_sha256(path, hex)`.

## Transport rules

- **HTTPS is required** for both the manifest and the artifact. The signature
  protects integrity, but plain HTTP lets a network attacker hide that an update
  exists at all. Loopback URLs are allowed for dev servers, and
  `UpdaterConfig::allow_insecure(true)` is an explicit opt-out for intranet mirrors.
  From the backend, the `Backend`'s own rule applies (HTTPS except loopback).
- **Size cap:** `max_artifact_bytes` (default 1 GiB) is enforced against
  `Content-Length`, the manifest's `size`, and while streaming, so a hostile or
  broken server can't fill the disk. A download of another size than the
  manifest said is refused.

## Capability

`/__update/check` needs the `Updater` capability (granted by default);
`/__update/install` and `/__update/restart` need `UpdaterInstall`, which is
**opt-in** because they download, replace the app and relaunch:

```rust
use elyra::security::Capability;

App::new()
    .updater(UpdaterConfig::new(PUBLIC_KEY, MANIFEST_URL, env!("CARGO_PKG_VERSION")))
    .allow_frontend(Capability::UpdaterInstall)   // enables the toast's install button
```

Both routes are rate-limited (install: once per 10s; the restart that follows
it isn't counted). See [security](security.md).

## What the app's release does

Generating keys, signing, notarizing, stapling and hosting are the release
pipeline's job. For a Developer ID update: sign with the hardened runtime,
notarize, staple, then zip the stapled bundle with
`ditto -c -k --keepParent --norsrc` (without `--norsrc`, AppleDouble `._`
files land inside the bundle on expansion and break its seal), and publish
the zip's size and SHA-256 with it.

[`App::updater`]: ../framework/src/app.rs
[`UpdaterConfig`]: ../framework/src/updater.rs
[`UpdaterConfig::new`]: ../framework/src/updater.rs
[`UpdaterConfig::from_backend`]: ../framework/src/updater.rs
[`UpdaterRuntime`]: ../framework/src/updater/runtime.rs
[`elyra::updater::bundle`]: ../framework/src/updater/bundle.rs

## Related

- [A Laravel backend](backend.md) — the server the update comes from
- [Security](security.md) — capabilities and the update's trust
- [Roadmap](roadmap.md) — distribution work still ahead
