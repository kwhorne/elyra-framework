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
let token = secrets.get("github_token")?;      // Option<Secret>
secrets.delete("github_token")?;               // idempotent
```

## `Secret`

Reads return a `Secret`, not a `String`. It derefs to `&str` (so it is used like
the `String` it replaces), its `Debug` prints `Secret(***)` so a stray log line
can't leak it, and it wipes its bytes on drop via `zeroize` instead of leaving
the plaintext in a freed allocation.

```rust
let token = secrets.get("github_token")?.ok_or("not signed in")?;
client.bearer(&token);          // Deref<Target = str>
client.bearer(token.expose());  // same thing, spelled out

// Copies escape the wipe — pass the `&str` along rather than cloning it.
let leaked: String = token.to_string();
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
