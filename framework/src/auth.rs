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
//! - A token obtained another way (OAuth's device grant in the browser) is
//!   adopted with [`Auth::sign_in_with_token`], which also chooses the server
//!   of a [`Backend::chosen_at_sign_in`] and, while signed in, switches to
//!   another identity without a sign-out between.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use parking_lot::{Mutex, RwLock};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::backend::{Backend, BackendError};
use crate::event::EventBus;

/// The channel sign-ins and sign-outs arrive on, as [`AuthState`].
pub const CHANNEL: &str = "elyra:auth";

/// The [`TokenStore`] key a [`Backend::chosen_at_sign_in`] remembers its
/// server under while the user is signed in. The token is beside it, under
/// `backend-token:<server>`, and the user's id under `backend-user:<server>`.
pub const SERVER_KEY: &str = "backend-server";

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
    /// The user as `GET /api/user` answered, when it has, or as
    /// [`Auth::sign_in_with_token`] was given it.
    pub user: Option<Value>,
    /// Why the user is signed out: `"signed-out"` (they asked), `"expired"`
    /// (the server said `401`).
    pub reason: Option<String>,
}

/// What changed about who's signed in, for what the app runs per user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    SignedIn,
    /// Signed in still, as another user or with another token
    /// (`sign_in_with_token` while signed in).
    Switched,
    SignedOut,
}

/// Told each [`Change`].
type Watcher = Arc<dyn Fn(Change) + Send + Sync>;

/// Signing in and out. Bound by `App` when it has a backend.
#[derive(Clone)]
pub struct Auth {
    inner: Arc<Inner>,
}

struct Inner {
    backend: Backend,
    store: Arc<dyn TokenStore>,
    device: String,
    user: RwLock<Option<Value>>,
    /// Told of each sign-in, switch and sign-out — the broadcaster.
    watchers: RwLock<Vec<Watcher>>,
    bus: Option<EventBus>,
    /// One sign-in, switch or sign-out at a time.
    changing: tokio::sync::Mutex<()>,
}

impl Inner {
    /// The token's name in the store: one per server. `None` before a
    /// server is chosen.
    fn key(&self) -> Option<String> {
        Some(token_key(&self.backend.base_url()?))
    }

    /// The signed-in user's id, kept beside the token: whose offline copy
    /// (RFC 0007) it is, known before the server can be asked.
    fn user_key(&self) -> Option<String> {
        Some(user_key(&self.backend.base_url()?))
    }
}

fn token_key(base: &str) -> String {
    format!("backend-token:{base}")
}

fn user_key(base: &str) -> String {
    format!("backend-user:{base}")
}

impl Auth {
    /// Signing in to `backend`, keeping the token in `store`. A token already
    /// there is put to use: the user stays signed in across restarts — for a
    /// backend chosen at sign-in, to the server they chose.
    pub fn new(
        backend: Backend,
        store: Arc<dyn TokenStore>,
        app: &str,
        bus: Option<EventBus>,
    ) -> Self {
        let inner = Arc::new(Inner {
            device: device_name(app),
            backend,
            store,
            user: RwLock::new(None),
            watchers: RwLock::new(Vec::new()),
            bus,
            changing: tokio::sync::Mutex::new(()),
        });
        if inner.backend.is_chosen_at_sign_in() {
            match inner.store.get(SERVER_KEY) {
                Ok(Some(base)) => inner.backend.choose_base(Some(&base)),
                Ok(None) => {}
                Err(e) => {
                    crate::debug!(target: "elyra::auth", "can't read the chosen server: {e}")
                }
            }
        }
        let stored = inner.key().map(|key| inner.store.get(&key));
        match stored {
            Some(Ok(Some(token))) => {
                inner.backend.use_token(Some(token));
                if let Some(Ok(Some(id))) = inner.user_key().map(|k| inner.store.get(&k)) {
                    inner.backend.use_identity(Some(&id));
                }
            }
            // A server remembered without its token: nobody's signed in to it.
            Some(Ok(None)) if inner.backend.is_chosen_at_sign_in() => {
                inner.backend.choose_base(None);
                let _ = inner.store.delete(SERVER_KEY);
            }
            Some(Ok(None)) | None => {}
            // No keychain to read: signed out, as if nothing were stored.
            Some(Err(e)) => {
                inner.backend.choose_base(None);
                crate::debug!(target: "elyra::auth", "can't read the stored sign-in: {e}")
            }
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
        let stored = match inner.key() {
            Some(key) => inner.store.set(&key, &token),
            None => Err("no server to keep it for".into()),
        };
        if let Err(e) = stored {
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
        // Someone else's offline copy isn't this user's to see; this user's
        // outbox, kept from before, carries on.
        inner.backend.forget_offline_others().await;
        inner.backend.resume_outbox();
        crate::info!(
            target: "elyra::auth",
            "signed in to {}",
            inner.backend.base_url().unwrap_or_default()
        );
        self.emit(None);
        self.tell(Change::SignedIn);
        Ok(user)
    }

    /// Sign in with a token obtained another way — OAuth's device grant
    /// through the browser (SSO, 2FA), say — and the user it belongs to, as
    /// the server describes them (what [`user`](Self::user) returns and
    /// `elyra:auth` carries). After it, everything runs as after
    /// [`sign_in`](Self::sign_in): broadcasting, abilities, notifications.
    ///
    /// - **The server:** for a [`Backend::chosen_at_sign_in`], `base_url`
    ///   chooses it (a trailing `/` trimmed), and it's remembered beside the
    ///   token. For a [`Backend::new`], it must be that server: an app with a
    ///   fixed server can't be pointed elsewhere.
    /// - **The token** is kept in the keychain as `sign_in` keeps one, so
    ///   the user stays signed in across restarts. The user value isn't: at
    ///   the next start it's read again from `user_route`, so point that at
    ///   a route that answers it (`/api/v1/me`).
    /// - **Already signed in, it's a switch**, to another workspace's token
    ///   or another user on the same server: the broadcaster reconnects with
    ///   the new token to the channels named from the new user, the
    ///   abilities are read again, everything live that read the backend
    ///   re-runs, the offline copy of reads is dropped (another token's
    ///   answers differ), and `elyra:auth` says `signedIn: true` once, with
    ///   the new user — never `false` between. A switch to another server
    ///   needs a sign-out first.
    /// - **Writes waiting in the outbox** (RFC 0007) stay with their user.
    ///   A switch that keeps the user's id but changes the token would send
    ///   them with the new one, so it's refused with `PendingChanges` (kind
    ///   `pending_changes`) until they're sent or discarded.
    pub async fn sign_in_with_token(
        &self,
        base_url: &str,
        token: &str,
        user: Value,
    ) -> Result<(), BackendError> {
        let inner = &self.inner;
        let _changing = inner.changing.lock().await;
        let base = crate::backend::normalise(base_url);
        if reqwest::Url::parse(&base)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
            .is_none()
        {
            return Err(BackendError::Http(crate::http::HttpError::InvalidUrl(
                base_url.to_owned(),
            )));
        }
        if !plausible(token) {
            return Err(BackendError::Other {
                status: 0,
                message: "that isn't a token".into(),
            });
        }
        let current = inner.backend.base_url();
        let signed_in = inner.backend.has_token();
        if !inner.backend.is_chosen_at_sign_in() {
            if current.as_deref() != Some(base.as_str()) {
                return Err(BackendError::Other {
                    status: 0,
                    message: format!(
                        "this app's server is {}, not {base}",
                        current.unwrap_or_default()
                    ),
                });
            }
        } else if let (true, Some(current)) = (signed_in, &current) {
            if *current != base {
                return Err(BackendError::Other {
                    status: 0,
                    message: format!(
                        "signed in to {current}: sign out before signing in to {base}"
                    ),
                });
            }
        }
        let identity = inner.backend.identity(&user);
        if signed_in {
            let before = self.current_identity();
            let same_token = inner.backend.current_token().as_deref() == Some(token);
            if !same_token && before.is_some() && before == identity {
                let waiting = inner.backend.outbox_unsent().await;
                if waiting > 0 {
                    return Err(BackendError::PendingChanges(waiting));
                }
            }
        }

        // Kept first: with no keychain, nothing changes.
        let (key, id_key) = (token_key(&base), user_key(&base));
        inner
            .store
            .set(&key, token)
            .map_err(BackendError::Keychain)?;
        if inner.backend.is_chosen_at_sign_in() {
            if let Err(e) = inner.store.set(SERVER_KEY, &base) {
                if !signed_in {
                    let _ = inner.store.delete(&key);
                }
                return Err(BackendError::Keychain(e));
            }
        }
        match &identity {
            Some(id) => {
                if let Err(e) = inner.store.set(&id_key, id) {
                    crate::debug!(target: "elyra::auth", "can't keep the user's id: {e}");
                }
            }
            None => {
                let _ = inner.store.delete(&id_key);
            }
        }

        inner.backend.choose_base(Some(&base));
        inner.backend.use_token(Some(token.to_owned()));
        *inner.user.write() = Some(user);
        inner.backend.use_identity(identity.as_deref());
        if signed_in {
            // Another token's answers aren't this one's; the outbox carries
            // on as the new user.
            inner.backend.forget_offline_reads().await;
            inner.backend.resume_outbox();
            crate::info!(target: "elyra::auth", "switched, on {base}");
            self.emit(None);
            self.tell(Change::Switched);
        } else {
            inner.backend.forget_offline_others().await;
            inner.backend.resume_outbox();
            crate::info!(target: "elyra::auth", "signed in to {base}");
            self.emit(None);
            self.tell(Change::SignedIn);
        }
        Ok(())
    }

    /// The signed-in user's identity, as known now: the cached user's, else
    /// the one kept beside the token.
    fn current_identity(&self) -> Option<String> {
        let inner = &self.inner;
        let cached = inner.user.read().clone();
        cached
            .and_then(|user| inner.backend.identity(&user))
            .or_else(|| {
                let key = inner.user_key()?;
                inner.store.get(&key).ok().flatten()
            })
    }

    /// Revoke the token on the server — best-effort: offline, it's forgotten
    /// all the same, and the user can revoke it from the web.
    ///
    /// Refused with `BackendError::PendingChanges` (kind `pending_changes`)
    /// while writes wait in the outbox (RFC 0007): they're this user's, and
    /// signing out would lose them. [`sign_out_discarding`](Self::sign_out_discarding)
    /// signs out anyway.
    pub async fn sign_out(&self) -> Result<(), BackendError> {
        let waiting = self.inner.backend.outbox_unsent().await;
        if waiting > 0 {
            return Err(BackendError::PendingChanges(waiting));
        }
        self.sign_out_now().await
    }

    /// Sign out, dropping the writes that haven't been sent.
    pub async fn sign_out_discarding(&self) -> Result<(), BackendError> {
        self.inner.backend.discard_outbox().await;
        self.sign_out_now().await
    }

    async fn sign_out_now(&self) -> Result<(), BackendError> {
        let inner = &self.inner;
        if inner.backend.has_token() {
            let revoke = inner.backend.routes().revoke.clone();
            if let Err(e) = inner.backend.delete(&revoke).send().await {
                crate::debug!(target: "elyra::auth", "couldn't revoke the token: {e}");
            }
        }
        // The next person at this machine sees nothing of theirs.
        inner.backend.forget_offline_copy().await;
        if let Some(user_key) = inner.user_key() {
            if let Err(e) = inner.store.delete(&user_key) {
                crate::debug!(target: "elyra::auth", "can't remove the stored user: {e}");
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
        let inner = &self.inner;
        if let (Some(id), Some(user_key)) = (inner.backend.identity(&user), inner.user_key()) {
            inner.backend.use_identity(Some(&id));
            if inner.store.get(&user_key).ok().flatten().as_deref() != Some(id.as_str()) {
                if let Err(e) = inner.store.set(&user_key, &id) {
                    crate::debug!(target: "elyra::auth", "can't keep the user's id: {e}");
                }
            }
        }
        Ok(user)
    }

    /// Run `f` on each sign-in, switch and sign-out.
    pub(crate) fn watch(&self, f: impl Fn(Change) + Send + Sync + 'static) {
        self.inner.watchers.write().push(Arc::new(f));
    }

    fn tell(&self, change: Change) {
        let watchers = self.inner.watchers.read().clone();
        for watcher in watchers {
            watcher(change);
        }
    }

    /// Drop the token everywhere, and say why.
    fn forget(&self, reason: &str) {
        self.tell(Change::SignedOut);
        self.forget_quietly();
        crate::info!(target: "elyra::auth", "signed out ({reason})");
        self.emit(Some(reason));
    }

    /// …and, for a backend chosen at sign-in, the server: the next start
    /// shows the sign-in screen, which asks for it again.
    fn forget_quietly(&self) {
        let inner = &self.inner;
        inner.backend.use_token(None);
        *inner.user.write() = None;
        if let Some(key) = inner.key() {
            if let Err(e) = inner.store.delete(&key) {
                crate::debug!(target: "elyra::auth", "can't remove the stored sign-in: {e}");
            }
        }
        if inner.backend.is_chosen_at_sign_in() {
            if let Err(e) = inner.store.delete(SERVER_KEY) {
                crate::debug!(target: "elyra::auth", "can't remove the chosen server: {e}");
            }
            inner.backend.choose_base(None);
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
    plausible(&token).then_some(token)
}

/// Whether `token` could be a bearer token: not empty, not a page, no
/// whitespace. A JWT (an OAuth access token) is up to a few KB.
fn plausible(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 8192
        && !token
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<')
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
