//! Secrets — OS keychain storage for tokens and API keys (feature `secrets`).
//!
//! [`Store`](crate::Store) writes `settings.json` in **clear text**, and API keys
//! were expected to come from process environment variables — neither is a place
//! for an OAuth refresh token. This module keeps secrets where the OS wants them:
//!
//! * macOS — Keychain
//! * Windows — Credential Manager
//! * Linux — Secret Service (GNOME Keyring / KWallet)
//!
//! ```ignore
//! use elyra::secrets::Secrets;
//!
//! let secrets = ctx.get::<Secrets>();
//! secrets.set("github_token", &token)?;
//! if let Some(token) = secrets.get("github_token")? { /* … */ }
//! secrets.delete("github_token")?;
//! ```
//!
//! Bind it with [`SecretsProvider`]. Secrets are deliberately **not** exposed over
//! IPC: a value that any script in the webview can read isn't a secret. Read them
//! in a `#[command]` and hand out only what the UI needs.
//!
//! ## A note on AI provider keys
//! Shipping a provider key inside a distributed desktop binary can't be made safe
//! — anyone with the app has the key. Keep those server-side and proxy through
//! your own backend; use this module for *user* credentials.

use keyring::Entry;

/// Errors from the platform credential store.
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("the platform keychain is unavailable: {0}")]
    Unavailable(String),
    #[error("keychain error: {0}")]
    Backend(String),
}

type Result<T> = std::result::Result<T, SecretsError>;

/// Namespaced access to the OS credential store.
#[derive(Clone)]
pub struct Secrets {
    service: String,
}

impl Secrets {
    /// Store secrets under `service` (usually the app name, so the OS groups them).
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// The service name secrets are stored under.
    pub fn service(&self) -> &str {
        &self.service
    }

    fn entry(&self, key: &str) -> Result<Entry> {
        Entry::new(&self.service, key).map_err(|e| SecretsError::Unavailable(e.to_string()))
    }

    /// Read a secret, or `None` when it isn't set.
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretsError::Backend(e.to_string())),
        }
    }

    /// Store (or replace) a secret.
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?
            .set_password(value)
            .map_err(|e| SecretsError::Backend(e.to_string()))
    }

    /// Remove a secret. Missing keys are not an error (delete is idempotent).
    pub fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(SecretsError::Backend(e.to_string())),
        }
    }

    /// Whether a secret exists.
    pub fn has(&self, key: &str) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    /// Read a secret, falling back to an environment variable (and migrating it
    /// into the keychain when found), so an app can move off env-var config
    /// without breaking existing installs.
    pub fn get_or_migrate_env(&self, key: &str, env_var: &str) -> Result<Option<String>> {
        if let Some(existing) = self.get(key)? {
            return Ok(Some(existing));
        }
        match std::env::var(env_var) {
            Ok(value) if !value.is_empty() => {
                self.set(key, &value)?;
                Ok(Some(value))
            }
            _ => Ok(None),
        }
    }
}

/// A [`Provider`](crate::Provider) that binds [`Secrets`] for the app.
///
/// ```no_run
/// use elyra::App;
/// use elyra::secrets::SecretsProvider;
/// App::new().provider(SecretsProvider::for_app("My App")).run().unwrap();
/// ```
pub struct SecretsProvider {
    service: Option<String>,
}

impl SecretsProvider {
    /// Namespace secrets under an explicit service name.
    pub fn for_app(service: impl Into<String>) -> Self {
        Self {
            service: Some(service.into()),
        }
    }

    /// Derive the service name from `App::about(..)`.
    pub fn from_about() -> Self {
        Self { service: None }
    }
}

impl Default for SecretsProvider {
    fn default() -> Self {
        Self::from_about()
    }
}

impl crate::Provider for SecretsProvider {
    fn register(&self, container: &mut crate::Container) {
        // `About` isn't bound yet at register time, so fall back to the exe name.
        let service = self.service.clone().unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "elyra-app".to_string())
        });
        container.bind(Secrets::new(service));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> String {
        format!("elyra-secrets-test-{}", std::process::id())
    }

    #[test]
    fn set_get_delete_roundtrip() {
        let secrets = Secrets::new(service());
        // CI containers often have no Secret Service; skip rather than fail.
        if secrets.set("token", "s3cret").is_err() {
            eprintln!("skipping: no platform keychain available");
            return;
        }
        assert_eq!(secrets.get("token").unwrap().as_deref(), Some("s3cret"));
        assert!(secrets.has("token").unwrap());

        secrets.set("token", "rotated").unwrap();
        assert_eq!(secrets.get("token").unwrap().as_deref(), Some("rotated"));

        secrets.delete("token").unwrap();
        assert_eq!(secrets.get("token").unwrap(), None);
        assert!(!secrets.has("token").unwrap());
        // Deleting again is a no-op, not an error.
        secrets.delete("token").unwrap();
    }

    #[test]
    fn missing_keys_read_as_none() {
        let secrets = Secrets::new(service());
        match secrets.get("definitely-not-set") {
            Ok(value) => assert_eq!(value, None),
            Err(_) => eprintln!("skipping: no platform keychain available"),
        }
    }

    #[test]
    fn env_migration_moves_a_value_into_the_keychain() {
        let secrets = Secrets::new(format!("{}-migrate", service()));
        if secrets.get("api_key").is_err() {
            eprintln!("skipping: no platform keychain available");
            return;
        }
        std::env::set_var("ELYRA_TEST_API_KEY", "from-env");
        let value = secrets
            .get_or_migrate_env("api_key", "ELYRA_TEST_API_KEY")
            .unwrap();
        assert_eq!(value.as_deref(), Some("from-env"));

        // Now it lives in the keychain, so the env var is no longer consulted.
        std::env::remove_var("ELYRA_TEST_API_KEY");
        assert_eq!(
            secrets
                .get_or_migrate_env("api_key", "ELYRA_TEST_API_KEY")
                .unwrap()
                .as_deref(),
            Some("from-env")
        );
        let _ = secrets.delete("api_key");
    }
}
