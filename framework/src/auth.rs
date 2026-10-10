//! Signing in to the Laravel backend (RFC 0005 step 3): a Sanctum token for
//! this device, kept in the OS keychain, never in the webview.
//!
//! ```ignore
//! let auth = ctx.get::<Auth>();
//! auth.sign_in("ada@example.com", "secret").await?;   // a 422 for bad credentials
//! let user: User = auth.user().await?;                // GET /api/user, cached
//! auth.sign_out().await?;                             // revoke, forget
//! ```
//!
//! - The token route gets `email`, `password` and `device_name` (this
//!   machine and app, so the user recognizes it on the web's devices page).
//! - The token is stored under the app and the backend's URL, and restored
//!   when the app starts: a signed-in user stays signed in.
//! - A `401` from any request forgets it and says so on `elyra:auth`.
//! - With no keychain to keep it in, sign-in fails — and the token the server
//!   just issued is revoked. There's no plain-text fallback.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use parking_lot::{Mutex, RwLock};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::backend::{Backend, BackendError};
use crate::event::EventBus;

/// The channel sign-ins and sign-outs arrive on, as [`AuthState`].
pub const CHANNEL: &str = "elyra:auth";

/// Where a token is kept: the OS keychain ([`KeychainTokens`]), or memory in
/// a test ([`MemoryTokens`]).
pub trait TokenStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, token: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// The OS keychain: Keychain on macOS, Credential Manager on Windows, Secret
/// Service on Linux.
pub struct KeychainTokens(crate::secrets::Secrets);

impl KeychainTokens {
    /// Keep tokens under `service` — the app's name.
    pub fn new(service: impl Into<String>) -> Self {
        Self(crate::secrets::Secrets::new(service))
    }
}

impl TokenStore for KeychainTokens {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.0
            .get(key)
            .map(|s| s.map(|s| s.expose().to_owned()))
            .map_err(|e| e.to_string())
    }

    fn set(&self, key: &str, token: &str) -> Result<(), String> {
        self.0.set(key, token).map_err(|e| e.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.delete(key).map_err(|e| e.to_string())
    }
}

/// Tokens in memory — what `TestApp` uses, so a test never touches the real
/// keychain. `failing()` behaves like a system with no keychain.
#[derive(Default)]
pub struct MemoryTokens {
    tokens: Mutex<HashMap<String, String>>,
    failing: bool,
}

impl MemoryTokens {
    pub fn new() -> Self {
        Self::default()
    }

    /// One that refuses to store anything, like a Linux box without Secret
    /// Service.
    pub fn failing() -> Self {
        Self {
            failing: true,
            ..Self::default()
        }
    }

    /// Start with `token` stored under `key` — a user who signed in before.
    pub fn with(self, key: impl Into<String>, token: impl Into<String>) -> Self {
        self.tokens.lock().insert(key.into(), token.into());
        self
    }

    /// What's stored now, for assertions.
    pub fn stored(&self, key: &str) -> Option<String> {
        self.tokens.lock().get(key).cloned()
    }
}

impl TokenStore for MemoryTokens {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.tokens.lock().get(key).cloned())
    }

    fn set(&self, key: &str, token: &str) -> Result<(), String> {
        if self.failing {
            return Err("no keychain on this system".into());
        }
        self.tokens.lock().insert(key.to_owned(), token.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.tokens.lock().remove(key);
        Ok(())
    }
}

/// Whether the user is signed in, and as whom — what `elyra:auth` carries and
/// the runtime's `auth` store holds.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    pub signed_in: bool,
    /// The user as `GET /api/user` answered, when it has.
    pub user: Option<Value>,
    /// Why the user is signed out: `"signed-out"` (they asked), `"expired"`
    /// (the server said `401`).
    pub reason: Option<String>,
}

/// Signing in and out. Bound by `App` when it has a backend.
#[derive(Clone)]
pub struct Auth {
    inner: Arc<Inner>,
}

struct Inner {
    backend: Backend,
    store: Arc<dyn TokenStore>,
    /// The token's name in the store: one per backend.
    key: String,
    device: String,
    user: RwLock<Option<Value>>,
    bus: Option<EventBus>,
}

impl Auth {
    /// Signing in to `backend`, keeping the token in `store`. A token already
    /// there is put to use: the user stays signed in across restarts.
    pub fn new(
        backend: Backend,
        store: Arc<dyn TokenStore>,
        app: &str,
        bus: Option<EventBus>,
    ) -> Self {
        let key = format!("backend-token:{}", backend.base_url());
        let inner = Arc::new(Inner {
            device: device_name(app),
            backend,
            store,
            key,
            user: RwLock::new(None),
            bus,
        });
        match inner.store.get(&inner.key) {
            Ok(Some(token)) => inner.backend.use_token(Some(token)),
            Ok(None) => {}
            // No keychain to read: signed out, as if nothing were stored.
            Err(e) => crate::debug!(target: "elyra::auth", "can't read the stored sign-in: {e}"),
        }
        // A `401` anywhere means the token is gone. Weak: the backend lives
        // in the auth it reports to.
        let weak: Weak<Inner> = Arc::downgrade(&inner);
        inner.backend.on_signed_out(move || {
            if let Some(inner) = weak.upgrade() {
                Auth { inner }.forget("expired");
            }
        });
        Auth { inner }
    }

    /// The device name the token is issued for: this machine and the app.
    pub fn device_name(&self) -> &str {
        &self.inner.device
    }

    pub fn is_signed_in(&self) -> bool {
        self.inner.backend.has_token()
    }

    /// Sign in with Laravel's token route, and read the user. A `422` (wrong
    /// credentials, a missing field) is the validation bag, per field.
    pub async fn sign_in(&self, email: &str, password: &str) -> Result<Value, BackendError> {
        let inner = &self.inner;
        let routes = inner.backend.routes().clone();
        let response = inner
            .backend
            .post(&routes.token)
            .body(&json!({ "email": email, "password": password, "device_name": inner.device }))
            .send()
            .await?;
        let token = token_from(&response.text()).ok_or_else(|| BackendError::Other {
            status: response.status(),
            message: "the token route answered something other than a token".into(),
        })?;
        if let Err(e) = inner.store.set(&inner.key, &token) {
            // Don't leave a token on the server that nothing will ever use.
            let _ = inner
                .backend
                .delete(&routes.revoke)
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await;
            return Err(BackendError::Keychain(e));
        }
        inner.backend.use_token(Some(token));
        let user = match self.fetch_user().await {
            Ok(user) => user,
            Err(e) => {
                self.forget_quietly();
                return Err(e);
            }
        };
        crate::info!(target: "elyra::auth", "signed in to {}", inner.backend.base_url());
        self.emit(None);
        Ok(user)
    }

    /// Revoke the token on the server — best-effort: offline, it's forgotten
    /// all the same, and the user can revoke it from the web.
    pub async fn sign_out(&self) -> Result<(), BackendError> {
        let inner = &self.inner;
        if inner.backend.has_token() {
            let revoke = inner.backend.routes().revoke.clone();
            if let Err(e) = inner.backend.delete(&revoke).send().await {
                crate::debug!(target: "elyra::auth", "couldn't revoke the token: {e}");
            }
        }
        self.forget("signed-out");
        Ok(())
    }

    /// The signed-in user (`GET /api/user`), read once and then cached.
    pub async fn user<T: DeserializeOwned>(&self) -> Result<T, BackendError> {
        let cached = self.inner.user.read().clone();
        let user = match cached {
            Some(user) => user,
            None => self.fetch_user().await?,
        };
        serde_json::from_value(user)
            .map_err(|e| BackendError::Http(crate::http::HttpError::Decode(e.to_string())))
    }

    /// The state now, without asking the server.
    pub fn state(&self) -> AuthState {
        AuthState {
            signed_in: self.is_signed_in(),
            user: self.inner.user.read().clone(),
            reason: None,
        }
    }

    async fn fetch_user(&self) -> Result<Value, BackendError> {
        let route = self.inner.backend.routes().user.clone();
        let user: Value = self.inner.backend.get(route).json().await?;
        *self.inner.user.write() = Some(user.clone());
        Ok(user)
    }

    /// Drop the token everywhere, and say why.
    fn forget(&self, reason: &str) {
        self.forget_quietly();
        crate::info!(target: "elyra::auth", "signed out ({reason})");
        self.emit(Some(reason));
    }

    fn forget_quietly(&self) {
        let inner = &self.inner;
        inner.backend.use_token(None);
        *inner.user.write() = None;
        if let Err(e) = inner.store.delete(&inner.key) {
            crate::debug!(target: "elyra::auth", "can't remove the stored sign-in: {e}");
        }
    }

    fn emit(&self, reason: Option<&str>) {
        if let Some(bus) = &self.inner.bus {
            let mut state = self.state();
            state.reason = reason.map(str::to_owned);
            let _ = bus.emit(CHANNEL, &state);
        }
    }
}

/// The token in a token route's answer: Laravel's example returns it as
/// text; an app may wrap it as JSON (`"…"`, `{ "token": "…" }`).
fn token_from(body: &str) -> Option<String> {
    let body = body.trim();
    let token = match serde_json::from_str::<Value>(body) {
        Ok(Value::String(token)) => token,
        Ok(Value::Object(map)) => map
            .get("token")
            .or_else(|| map.get("plainTextToken"))
            .and_then(Value::as_str)?
            .to_owned(),
        // A Sanctum token (`1|abc…`) isn't JSON.
        _ => body.to_owned(),
    };
    let plausible = !token.is_empty()
        && token.len() <= 1024
        && !token
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<');
    plausible.then_some(token)
}

/// `<host> · <app>`: what the user sees on the web's list of devices.
fn device_name(app: &str) -> String {
    let host = hostname()
        .map(|h| h.trim_end_matches(".local").to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this computer".into());
    let app = if app.is_empty() { "Elyra" } else { app };
    format!("{host} · {app}")
}

#[cfg(unix)]
fn hostname() -> Option<String> {
    extern "C" {
        fn gethostname(name: *mut std::ffi::c_char, len: usize) -> std::ffi::c_int;
    }
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for `len` bytes; gethostname writes at most
    // that many and we stop at the first NUL.
    let ok = unsafe { gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    if !ok {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..end].to_vec()).ok()
}

#[cfg(not(unix))]
fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_from_each_shape_a_route_may_answer() {
        assert_eq!(token_from("1|abcDEF123").as_deref(), Some("1|abcDEF123"));
        assert_eq!(token_from("  1|abc \n").as_deref(), Some("1|abc"));
        assert_eq!(token_from(r#""1|abc""#).as_deref(), Some("1|abc"));
        assert_eq!(token_from(r#"{"token":"1|abc"}"#).as_deref(), Some("1|abc"));
        assert_eq!(
            token_from(r#"{"plainTextToken":"1|abc"}"#).as_deref(),
            Some("1|abc")
        );
        assert_eq!(token_from(""), None);
        assert_eq!(
            token_from("<!DOCTYPE html><html>"),
            None,
            "an HTML page isn't a token"
        );
        assert_eq!(token_from(r#"{"user":1}"#), None);
    }

    #[test]
    fn the_device_is_named_for_the_machine_and_the_app() {
        let name = device_name("CRM");
        assert!(name.ends_with(" · CRM"), "{name}");
        assert!(!name.contains(".local"), "{name}");
        assert!(device_name("").ends_with(" · Elyra"));
    }
}
