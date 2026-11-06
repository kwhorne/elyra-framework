# RFC 0024 — Updates from the backend, and whole bundles on macOS

**Status:** implemented (2026-11-06) · **Target:** 0.36.0 · builds on [RFC 0021](0021-backend-chosen-at-sign-in.md) and [RFC 0022](0022-multiple-servers.md) · see [Decisions](#decisions)

## Summary

Félagi Desktop is signed with a Developer ID and notarized, and it ships with
a Félagi licence: it's distributed from **each customer's own Félagi server**,
behind sign-in, not from a public URL. The updater can't serve it today: its
manifest URL is fixed when the app is built, and installing replaces the
executable inside the signed bundle.

This RFC asks the signed-in user's server instead, and replaces the whole
`.app`, trusted by its Developer ID:

```rust
App::new()
    .backend(Backend::chosen_at_sign_in().multiple_servers())
    .updater(
        UpdaterConfig::from_backend("/api/desktop/update", env!("CARGO_PKG_VERSION"))
            .developer_id("52AT44QFCM", "works.felagi.desktop")
            .auto_check(true),
    )
    .allow_frontend(Capability::UpdaterInstall)
```

```http
GET https://acme.felagi.no/api/desktop/update?platform=macos-aarch64&version=1.4.0
Authorization: Bearer …

200 { "version": "1.5.0", "notes": "…", "released_at": "2026-11-06",
      "url": "https://acme.felagi.no/desktop/Felagi-1.5.0-macos-aarch64.zip",
      "size": 31457280, "sha256": "…" }
204 (up to date)
```

The toast offers **Install** (download, verify, move into place), then
**Restart**: two steps, so a chat app isn't restarted mid-sentence.

## Motivation

- **The server is chosen at sign-in** (RFC 0021), and there may be several
  (RFC 0022). A URL fixed at build time can't point at it, and a public URL
  would hand a licensed app to anyone.
- **The download needs the user's token**, and must never carry it to
  another host.
- **A signed bundle can't be patched.** The seal covers `Info.plist`, the
  resources and every executable: an executable written into the bundle
  breaks it, Gatekeeper refuses the app, and macOS kills a process whose
  signed executable is overwritten under it. The whole `.app` has to be
  replaced, by moving, not writing.
- **The Developer ID is already the trust.** Gatekeeper checks it on every
  launch; a second, separate ed25519 key is one more secret to keep for the
  same answer. The team's other product, eterm, updates exactly this way.

## Current state (0.35)

- `UpdaterConfig::new(public_key, manifest_url, version)`: a manifest per
  platform `{ url, signature }`, fetched with `ureq`, the artifact verified by
  ed25519, streamed to disk with a size cap.
- `apply_and_relaunch` replaced a loose executable, or on macOS a whole bundle
  from a zip (checked with `codesign --verify --strict` and the installed
  bundle's identifier), and relaunched in the same step.
- The toast: *Install & restart*, one step. `/__update/check`,
  `/__update/install`; `UpdaterInstall` opt-in, once per 10 s.

## Design

### Where it asks: `UpdaterConfig::from_backend(path, version)`

- **`GET {active server}{path}?platform={target}&version={current}`**, built
  with `Backend::get(..).no_offline()`: the active server's base, its token,
  `401` signing out of it as any request does, `NoServer` before a sign-in.
  `target` is `Updater::current_target()` (`macos-aarch64`).
- **`204` is up to date.** `200` is `{ version, notes?, released_at?, url,
  size?, sha256, signature? }`, optionally inside `{ "data": … }`. A version
  no newer than the running one is up to date. `sha256` is required.
- **The origin rule.** `url` is resolved against the server (an absolute URL,
  or a path) and must have the active server's origin: scheme, host, port.
  Otherwise the check fails with `Error::ForeignOrigin` and nothing is sent
  there. It's checked again just before the download, against the server
  active then, whose token is the one sent.
- **Signed out, no check**: nothing is sent; a manual check answers
  `{ available: false, error }`, the schedule stays quiet.
- **A server switch** (RFC 0022): every check reads the active server and
  token when it runs; `Auth::watch` wakes the schedule after a sign-in, a
  switch or a sign-out. An offer from the old server that the new one doesn't
  make is withdrawn with `up-to-date`.

### How it trusts: `.developer_id(team_id, bundle_id)`

1. The download's **size** (when given) and **SHA-256** match the manifest.
2. It's a zip (by its magic); `ditto -x -k` expands it into a private
   directory (`0700`); it holds exactly one `.app`.
3. **`codesign --verify --strict --deep`** passes: nothing in the bundle,
   nested code included, changed since it was signed.
4. **`TeamIdentifier`** is the team, **`Identifier`** the bundle id
   (`codesign -dv`), so a refusal says which was wrong.
5. It satisfies **the Developer ID requirement** narrowed to them:
   `anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6]
   and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate
   leaf[subject.OU] = "<team>" and identifier "<bundle id>"` — the chain is
   Apple's, not a self-made certificate naming the team.
6. Its `CFBundleShortVersionString`, when semver, is newer than the running
   version: a server can't hand back an older signed release. (Not for a
   manifest URL with ed25519 alone, which works as before.)

The decision (steps 3–5) is a pure function, `bundle::judge(signature,
developer_id)`, over what the system reported (`CodeSignature`). ed25519
stays, as an alternative or in addition (`.public_key(..)`); the matrix is in
[the updater's docs](../updater.md#trust). `from_backend` with neither is a
configuration error. Unverified bytes are never moved out of the private
directory, which is removed on every path.

### Applying a bundle

- **The running bundle** is found from `current_exe()`: `X.app/Contents/MacOS/x`.
- **Refused, before anything is downloaded**, with a sentence the toast
  shows: running from a build (`target/{debug,release}`, a triple between, a
  `rata bundle` there), not inside an `.app`, translocated
  (`AppTranslocation`), or in a folder this user can't write (a disk image
  under `/Volumes` says so).
- **Moved, not written**: the installed bundle is renamed aside
  (`.X.app.old-<pid>`, hidden, not ending in `.app`), the new one renamed in
  (copied with `ditto` across volumes), the old one put back if that fails,
  and removed if it didn't. The quarantine attribute is cleared from the new
  bundle, which was verified as Gatekeeper would. Leftovers of an interrupted
  swap are removed at the next start.
- **`ready`, then `relaunch()`**: a detached `sh` waits for this process to
  exit and runs `open -n` on the bundle; the app quits as the tray's Quit
  does. Waiting matters to a single-instance app, whose new copy would
  otherwise hand its launch to the old one.
- **Other OSes** keep today's behaviour: the artifact is the executable,
  swapped in place, verified by ed25519. A Developer ID alone can't verify
  anything there, so installing is refused with a sentence that says so.

All of it that asks the system — `ditto`, `codesign`, `PlistBuddy`, extended
attributes, renames, the relaunch — is behind `bundle::BundleSystem`;
`NativeSystem` is the real one, and the tests pass fakes.

### Schedule and toast

- With `auto_check`, from the backend: 10 s after start, 3 s after each
  sign-in, switch or sign-out, every 24 h. The same version isn't offered
  twice in a row except daily. From a manifest URL: once at start, as before.
- `elyra:update` phases are unchanged — `available`, `downloading`, `ready`,
  `error`, `up-to-date` — with a new `restart: "ask" | "now"`.
- **Two steps** by default from the backend (`relaunch_when_asked`):
  *Install* → *↓ Downloading… n%* → *✓ v1.5.0 is installed. Restart to use
  it.* **Restart** / **Later**. `POST /__update/restart` relaunches;
  `restartToUpdate()` in the runtime. A manual `checkForUpdate()` after an
  install shows the ready toast (`UpdateCheck.ready`).
- **The toast's words** are English by default; `setUpdateTexts({..})`
  replaces any of them, and an app's catalog may translate them as
  `elyra.update.<key>`.

## Security

- **The token stays home.** Only the active server's origin is ever sent
  it, checked at the check and again at the download.
- **No unverified install.** Size, SHA-256, then the code signature (and
  ed25519, when configured) before anything moves; the install target is
  checked before anything downloads.
- **The SHA-256 comes from the same server as the zip**, so it catches a
  broken download, not a hostile server. The Developer ID is what a
  compromised server can't forge; the version check stops it serving an
  older signed release.
- **`UpdaterInstall` stays opt-in** and now covers `/__update/restart`; the
  restart isn't counted against the install's once-per-10 s.

## Alternatives considered

- **A public manifest URL per customer.** Hands the licensed app to anyone
  who finds it, and the URL still isn't known at build time.
- **The Security framework (`SecStaticCodeCheckValidity`) instead of
  `codesign`.** What eterm does; it needs `core-foundation` and
  `security-framework-sys`. `codesign` makes the same check with the same
  requirement, and is already what the existing bundle path uses. A
  `BundleSystem` on the Security framework can replace `NativeSystem` later
  without touching the decisions.
- **The `zip` crate instead of `ditto`.** Must handle symlinks, modes and
  AppleDouble files exactly; `ditto` is Apple's own and is what produced the
  zip.
- **Restarting at once, as before.** Fine for a viewer, wrong for a chat or a
  terminal; it stays available (`relaunch_when_asked(false)`).

## As built

- **`UpdaterConfig::from_backend(path, version)`** (feature `backend`),
  **`.developer_id(team, bundle_id)`**, **`.public_key(key)`**,
  **`.relaunch_when_asked(bool)`**; new fields `backend_path`,
  `developer_id`, `relaunch_when_asked`. `Updater::for_version(v)`, an
  updater without a key; `verify_sha256(path, hex)`; `UpdateInfo` gains
  `sha256`, `size`, `released_at` (and a URL manifest may carry `sha256`).
- **`elyra::updater::UpdaterRuntime`**, in the container: `check()`,
  `check_for_frontend()`, `announce(force)`, `install_target()`,
  `download(&info, progress)`, `install()`, `relaunch()`, `set_quit(..)`,
  `wake()`; `with_system(..)` and `with_executable(..)` for tests. Its fields
  are no longer public.
- **`elyra::updater::bundle`**: `DeveloperId`, `CodeSignature`, `BundleInfo`,
  `BundleSystem`, `NativeSystem`, `installed_bundle`, `from_build`, `judge`,
  `judge_intact`, `install_bundle`, `swap`, `sweep_leftovers`.
- **`elyra::updater::feed`**: `Release`, the backend's answer.
- New `Error` variants: `Config`, `SignedOut`, `ForeignOrigin`, `Digest`,
  `SizeMismatch`, `CodeSignature`, `NotNewer`, `NotInstallable`, `Busy`,
  `NothingInstalled`.
- `/__update/restart` under `UpdaterInstall`; `UpdateCheck` gains `ready` and
  `restart`.
- Runtime: `restartToUpdate()`, `setUpdateTexts()`, `UpdateTexts`; the toast
  handles `restart` and `up-to-date`.
- **Tests:** `framework/tests/updater_backend.rs` against `HttpFake` (200,
  204, a release no newer, signed out, a foreign origin in four shapes, the
  token on the manifest and the download, a SHA mismatch leaving nothing, a
  size mismatch, a switch between servers, the schedule's offer and its
  withdrawal, an install from a build refused before downloading, and on
  macOS a full two-step install and three refused signatures with a fake
  system); unit tests in `updater::bundle` (`judge` for wrong team, wrong
  bundle id, unsigned, ad hoc, a self-made certificate, altered; build,
  translocated and bundle paths; the swap and its rollback; and the real
  `codesign` against installed apps when present), `updater::runtime` (the
  install target per OS, the schedule), `updater::feed` (the origin rule);
  `runtime/src/update.test.ts`; `framework/tests/shell.rs` for the restart
  route's capability.

## Open questions

1. **Names:** `from_backend` / `developer_id` / `relaunch_when_asked` /
   `restartToUpdate`, or closer to eterm's (`relaunch`)?
2. **Delta or per-architecture fallbacks** (`macos-universal` when there's no
   `macos-aarch64`)? Recommended: the server decides; it gets `platform`.
3. **Windows and Linux from the backend** with a Developer ID equivalent
   (Authenticode)? Recommended: later, when an app ships there.
4. **Keeping the old bundle** for a rollback rather than removing it?
   Recommended: no; the previous release is on the server.

## Decisions

Built as recommended, to be confirmed before 0.36.0:

1. **The names above.**
2. **No fallback platforms** in the framework.
3. **Windows and Linux** keep ed25519; a Developer ID alone is refused there.
4. **The old bundle is removed** once the new one is in.
