# Secrets

Feature-gated behind `secrets`. Stores credentials in the OS keychain instead of
the clear-text settings file.

```toml
elyra = { version = "0.5", features = ["secrets"] }
```

| Platform | Backend |
|---|---|
| macOS | Keychain |
| Windows | Credential Manager |
| Linux | Secret Service (GNOME Keyring / KWallet) |

```rust
use elyra::secrets::{Secrets, SecretsProvider};

App::new().provider(SecretsProvider::for_app("My App"))

// in a #[command]
let secrets = ctx.get::<Secrets>();
secrets.set("github_token", &token)?;
let token = secrets.get("github_token")?;      // Option<String>
secrets.delete("github_token")?;               // idempotent
```

Migrating away from environment variables:

```rust
// Reads the keychain, falls back to $GITHUB_TOKEN and stores it for next time.
let token = secrets.get_or_migrate_env("github_token", "GITHUB_TOKEN")?;
```

## Deliberately not exposed to the frontend

There is no `/__secrets/*` route: a value any script in the webview can read isn't
a secret. Read secrets in a `#[command]` and return only what the UI needs.

## AI provider keys

A key inside a distributed desktop binary is a key your users have. Keep provider
keys server-side and proxy requests through your own backend; use `Secrets` for
*user* credentials (OAuth tokens, personal access tokens).

## Related

- [Store](store.md) — non-secret settings · [Security](security.md)
