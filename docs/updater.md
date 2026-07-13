# Auto-updater

Feature-gated behind `updater`. The security model mirrors Tauri's: releases are
published as an artifact plus an **ed25519 signature**, listed in a JSON
manifest, and the app ships the matching public key. An update is only ever
installed after its downloaded bytes verify against that key — so a compromised
release server still can't push a malicious binary.

```toml
elyra = { version = "0.1", features = ["updater"] }
```

## The built-in update component (recommended)

Most apps don't need the low-level API. Call [`App::updater`] with an
[`UpdaterConfig`] and the framework wires up the whole flow — a silent startup
check, two IPC endpoints, progress events, and a themed **update toast** in the
frontend (rendered by `@elyra/runtime`):

```rust
use elyra::{App, UpdaterConfig};

App::new()
    .title("MyApp")
    .updater(UpdaterConfig::new(
        PUBLIC_KEY_B64,
        "https://releases.example.com/latest.json",
        env!("CARGO_PKG_VERSION"),
    ))
    .run()
```

What you get:

- **Startup check** — on launch the shell checks the manifest silently. If a
  newer release exists it emits an `elyra:update` event and the toast appears:
  *↑ Update available: vX* with **What's new**, **Install & restart**, **Later**.
  Disable with `.auto_check(false)`.
- **Install** — clicking *Install & restart* downloads + verifies the artifact,
  streaming *↓ Downloading… n%*, then replaces the binary and relaunches.
- **Manual check** — from the frontend, `checkForUpdate()` returns an
  `UpdateCheck` and shows the toast if an update is available; wire it to a
  button or menu item. `installUpdate()` and `dismissUpdate()` are also exported.

### Endpoints & events

| Path | Purpose |
|---|---|
| `GET /__update/check` | run a check, return `{ available, version, notes, error? }` |
| `POST /__update/install` | download + verify + apply in the background |

Progress is pushed on the **`elyra:update`** channel as
`{ phase, version?, notes?, progress?, message? }`, where `phase` is one of
`available` / `downloading` / `ready` / `error` / `up-to-date`. `@elyra/runtime`
subscribes to it automatically.

## Checking for updates (low-level API)

```rust
use elyra::updater::{Updater, UpdateStatus};

let updater = Updater::new(PUBLIC_KEY_B64, env!("CARGO_PKG_VERSION"))?;
let target = Updater::current_target();          // e.g. "macos-aarch64"

match updater.check("https://releases.example.com/latest.json", &target)? {
    UpdateStatus::UpToDate => {}
    UpdateStatus::Available(info) => {
        let staged = updater.download_verified(&info)?; // signature-checked
        // ...then apply + relaunch (platform-specific).
    }
}
```

## The manifest

```json
{
  "version": "1.2.0",
  "notes": "Bug fixes",
  "platforms": {
    "macos-aarch64": {
      "url": "https://releases.example.com/MyApp-1.2.0-macos-aarch64.tar.gz",
      "signature": "<base64 ed25519 signature of the artifact>"
    }
  }
}
```

## API

- `Updater::new(public_key_b64, current_version)` — parse the key + version.
- `Updater::current_target()` — `"{os}-{arch}"`.
- `evaluate(manifest_json, target)` — pure: parse + semver compare, pick the
  target platform. Returns `UpdateStatus`.
- `verify(data, signature_b64)` — ed25519 verification with the bundled key.
- `check(manifest_url, target)` — HTTP fetch + `evaluate`.
- `download_verified(info)` — download the artifact, verify its signature, stage
  it to a temp file. Never returns unverified bytes.

## What's verified vs. what needs infra

`evaluate` (manifest + semver) and `verify` (ed25519) are pure and unit-tested.
`check` / `download_verified` do real HTTP. **Applying** the update —
`Updater::apply_and_relaunch` — replaces the running executable in place and
re-execs it. It assumes the artifact **is** the app binary (Elyra's
single-binary model) and is not exercised in tests; it's the one step that
touches the live environment.

> **Signing.** Replacing a code-signed binary invalidates its signature, so apps
> distributed through Gatekeeper must be re-signed (Developer ID + notarization)
> as part of releasing. Generating keys/signatures and hosting the manifest is
> your release pipeline's job. See the [roadmap](roadmap.md).

[`App::updater`]: ../framework/src/app.rs
[`UpdaterConfig`]: ../framework/src/updater.rs

## Related

- [Roadmap](roadmap.md) — distribution work still ahead
