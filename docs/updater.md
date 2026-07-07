# Auto-updater

Feature-gated behind `updater`. The security model mirrors Tauri's: releases are
published as an artifact plus an **ed25519 signature**, listed in a JSON
manifest, and the app ships the matching public key. An update is only ever
installed after its downloaded bytes verify against that key — so a compromised
release server still can't push a malicious binary.

```toml
elyra = { version = "0.1", features = ["updater"] }
```

## Checking for updates

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
`check`/`download_verified` do real HTTP. **Applying** the update — replacing the
running binary and relaunching — is inherently environment-specific and is left
as an integration step, not provided as an untested helper.

Generating keys/signatures and hosting the manifest is your release pipeline's
job (e.g. in CI, alongside Developer ID signing + notarization).

## Related

- [Roadmap](roadmap.md) — distribution work still ahead
