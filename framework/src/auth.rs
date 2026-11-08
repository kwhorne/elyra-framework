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
//! - With [`Backend::multiple_servers`] (RFC 0022) the user stays signed in
//!   to several servers, one active: [`Auth::accounts`] lists them,
//!   [`Auth::switch_to`] changes the active one, [`Auth::sign_out_of`]
//!   leaves one and [`Auth::sign_out_all`] all of them.
//! - With [`Backend::token_per_workspace`] (RFC 0031) each workspace on a
//!   server has its own token, kept from its own `sign_in_with_token`:
//!   [`Account::workspaces`] lists them, [`Auth::switch_workspace`] puts
//!   another in use, [`Auth::sign_out_of_workspace`] leaves one, and a `401`
//!   ends that workspace rather than the server.

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

/// The [`TokenStore`] key the servers a user is signed in to are listed
/// under, as a JSON array of their URLs, with [`Backend::multiple_servers`]
/// (RFC 0022). [`SERVER_KEY`] is the active one of them.
pub const SERVERS_KEY: &str = "backend-servers";

/// The channel the list of servers arrives on, as [`AccountsState`], each
/// time it changes: a sign-in, a switch, a sign-out of one (RFC 0022).
pub const ACCOUNTS_CHANNEL: &str = "elyra:accounts";

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

/// The app's [`Secrets`](crate::secrets::Secrets) as the token store:
/// `App::token_store(Arc::new(Secrets::fake()))` in a test that wants to see
/// each server's token where the app's other secrets are.
impl TokenStore for crate::secrets::Secrets {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        crate::secrets::Secrets::get(self, key)
            .map(|s| s.map(|s| s.expose().to_owned()))
            .map_err(|e| e.to_string())
    }

    fn set(&self, key: &str, token: &str) -> Result<(), String> {
        crate::secrets::Secrets::set(self, key, token).map_err(|e| e.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        crate::secrets::Secrets::delete(self, key).map_err(|e| e.to_string())
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
    /// The server requests go to: the active one (RFC 0022). `None` while
    /// signed out of a server chosen at sign-in.
    pub server: Option<String>,
}

/// A server the user is signed in to (RFC 0022), as [`Auth::accounts`] lists
/// it and `elyra:accounts` carries it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// `https://acme.felagi.no`, without a trailing `/`.
    pub server: String,
    /// The user's id there (at [`Backend::user_id_path`], else their email),
    /// as kept beside the token.
    pub user_id: Option<String>,
    /// The user as that server described them, when known this run: given
    /// to [`Auth::sign_in_with_token`], or read from `user_route` while the
    /// server was active. Not kept across restarts (RFC 0021).
    pub user: Option<Value>,
    /// Whether requests go there now.
    pub active: bool,
    /// Its writes waiting to be sent (RFC 0022): saved offline, sent once
    /// that server answers and is active. Counted whether it's active or
    /// not; `0` without [`Backend::offline`].
    pub pending: u32,
    /// Its writes the server refused, waiting for the user.
    pub failed: u32,
    /// Its workspaces, each with its own token, with
    /// [`Backend::token_per_workspace`] (RFC 0031); empty without.
    pub workspaces: Vec<Workspace>,
}

/// A workspace on a server, with a token of its own (RFC 0031), as
/// [`Account::workspaces`] lists it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    /// Its id, at [`Backend::token_per_workspace`]'s path in the user value.
    pub id: String,
    /// Who the user is there, as the server described them at the
    /// workspace's sign-in: its name and the user's role with it. Kept
    /// across restarts.
    pub user: Option<Value>,
    /// Whether its token is the one in use on the server.
    pub active: bool,
    /// Its writes waiting to be sent, and those the server refused.
    pub pending: u32,
    pub failed: u32,
}

/// What `elyra:accounts` carries: the servers, and the active one.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountsState {
    pub accounts: Vec<Account>,
    pub active: Option<String>,
    /// The server just left, when one was: signed out of, or its token
    /// refused (`401`).
    pub signed_out_of: Option<String>,
    /// Why: `"signed-out"` or `"expired"`, as in [`AuthState`].
    pub reason: Option<String>,
    /// The workspace just left on `signed_out_of`, when one was and the
    /// server is still signed in to (RFC 0031): left, or its token refused.
    pub signed_out_of_workspace: Option<String>,
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
    /// `.multiple_servers()` (RFC 0022): the servers signed in to, in the
    /// order they were, and the users of those not active now.
    servers: RwLock<Vec<String>>,
    parked: Mutex<HashMap<String, Value>>,
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

impl Inner {
    fn multiple(&self) -> bool {
        self.backend.allows_multiple_servers()
    }

    /// Keep the list of servers signed in to.
    fn save_servers(&self) {
        let list = self.servers.read().clone();
        let outcome = if list.is_empty() {
            self.store.delete(SERVERS_KEY)
        } else {
            self.store.set(
                SERVERS_KEY,
                &serde_json::to_string(&list).unwrap_or_default(),
            )
        };
        if let Err(e) = outcome {
            crate::debug!(target: "elyra::auth", "can't keep the list of servers: {e}");
        }
    }

    /// The servers signed in to, as kept, else the active one alone (as
    /// before RFC 0022).
    fn kept_servers(&self) -> Vec<String> {
        match self.store.get(SERVERS_KEY) {
            Ok(Some(list)) => serde_json::from_str::<Vec<String>>(&list).unwrap_or_default(),
            _ => match self.store.get(SERVER_KEY) {
                Ok(Some(base)) => vec![crate::backend::normalise(&base)],
                _ => Vec::new(),
            },
        }
    }

    fn stored(&self, key: &str) -> Option<String> {
        self.store.get(key).ok().flatten()
    }
}

fn token_key(base: &str) -> String {
    format!("backend-token:{base}")
}

/// The workspace the user is in on `base`, when the offline copy is kept per
/// workspace (`Backend::offline_scope_path`).
fn scope_key(base: &str) -> String {
    format!("backend-scope:{base}")
}

/// Every workspace on `base` the user has had a copy or an outbox in.
fn scopes_key(base: &str) -> String {
    format!("backend-scopes:{base}")
}

/// Whose offline copy and outbox: the user, in their workspace when there's
/// one. The owner hash is made from it, so each workspace has its own.
fn scoped(id: &str, scope: Option<&str>) -> String {
    match scope {
        Some(scope) => format!("{id}\u{1f}{scope}"),
        None => id.to_owned(),
    }
}

fn user_key(base: &str) -> String {
    format!("backend-user:{base}")
}

/// A workspace's own token on `base` (RFC 0031).
fn workspace_token_key(base: &str, workspace: &str) -> String {
    format!("backend-token:{base}#{workspace}")
}

/// Who the user is in a workspace on `base`, as JSON (RFC 0031).
fn workspace_user_key(base: &str, workspace: &str) -> String {
    format!("backend-workspace:{base}#{workspace}")
}

/// The workspaces on `base` with a token of their own, as a JSON array.
fn workspaces_key(base: &str) -> String {
    format!("backend-workspaces:{base}")
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
            servers: RwLock::new(Vec::new()),
            parked: Mutex::new(HashMap::new()),
            watchers: RwLock::new(Vec::new()),
            bus,
            changing: tokio::sync::Mutex::new(()),
        });
        // Several servers (RFC 0022): each kept with its token, the active
        // one under `backend-server`. A list from before is the active
        // server alone; a server whose token is gone isn't signed in to.
        if inner.multiple() {
            let kept = inner.kept_servers();
            let mut list: Vec<String> = Vec::new();
            for base in kept {
                if !list.contains(&base) && inner.stored(&token_key(&base)).is_some() {
                    list.push(base);
                }
            }
            let active = inner
                .stored(SERVER_KEY)
                .map(|b| crate::backend::normalise(&b))
                .filter(|b| list.contains(b))
                .or_else(|| list.first().cloned());
            *inner.servers.write() = list;
            inner.save_servers();
            match &active {
                Some(base) => {
                    if inner.stored(SERVER_KEY).as_deref() != Some(base.as_str()) {
                        let _ = inner.store.set(SERVER_KEY, base);
                    }
                }
                None => {
                    let _ = inner.store.delete(SERVER_KEY);
                }
            }
        }
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
                    // The workspace it was in: offline at a start, its copy answers.
                    let scope = inner
                        .backend
                        .base_url()
                        .filter(|_| inner.backend.offline_is_scoped())
                        .and_then(|base| inner.stored(&scope_key(&base)));
                    inner
                        .backend
                        .use_identity(Some(&scoped(&id, scope.as_deref())));
                }
            }
            // A server remembered without its token: nobody's signed in to it.
            Some(Ok(None)) if inner.backend.is_chosen_at_sign_in() => {
                inner.backend.choose_base(None);
                let _ = inner.store.delete(SERVER_KEY);
                inner.servers.write().clear();
                if inner.multiple() {
                    inner.save_servers();
                }
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
        let auth = Auth { inner };
        auth.restore_workspace_users();
        auth
    }

    /// With a token per workspace (RFC 0031), who the user is in the
    /// workspace in use on each server, as kept: known at a start, before
    /// the server is asked.
    fn restore_workspace_users(&self) {
        let inner = &self.inner;
        if !inner.backend.has_token_per_workspace() {
            return;
        }
        let active = self.active_server();
        for server in self.servers() {
            let Some(user) = inner
                .stored(&scope_key(&server))
                .and_then(|w| self.workspace_user(&server, &w))
            else {
                continue;
            };
            if active.as_deref() == Some(server.as_str()) {
                *inner.user.write() = Some(user);
            } else {
                inner.parked.lock().insert(server, user);
            }
        }
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
        if self.current_identity().is_some() {
            inner
                .backend
                .forget_offline_others(&self.signed_in_identities())
                .await;
        }
        inner.backend.resume_outbox();
        crate::info!(
            target: "elyra::auth",
            "signed in to {}",
            inner.backend.base_url().unwrap_or_default()
        );
        self.emit(None);
        self.tell(Change::SignedIn);
        self.emit_accounts(None, None);
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
    ///   re-runs, the offline copy of that server's reads is dropped (another
    ///   token's answers differ), and `elyra:auth` says `signedIn: true` once,
    ///   with the new user — never `false` between. A switch to another
    ///   server needs a sign-out first, unless the backend has
    ///   [`Backend::multiple_servers`].
    /// - **Another server, with [`Backend::multiple_servers`]** (RFC 0022):
    ///   it's added to the servers signed in to and becomes the active one,
    ///   as [`switch_to`](Self::switch_to) would make it. The others stay
    ///   signed in, with their tokens, offline copies and waiting writes.
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
        // Signed in to another server now: an error, unless several may be.
        let elsewhere = signed_in && current.as_deref().is_some_and(|c| c != base);
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
        } else if let (true, false, Some(current)) = (elsewhere, inner.multiple(), &current) {
            return Err(BackendError::Other {
                status: 0,
                message: format!("signed in to {current}: sign out before signing in to {base}"),
            });
        }
        let identity = inner.backend.identity(&user);
        let scope = inner.backend.offline_scope(&user);
        let offline_identity = identity.as_deref().map(|id| scoped(id, scope.as_deref()));
        // Another workspace's copy is its own: kept, not dropped.
        let keep_reads = inner.backend.offline_is_scoped() && scope.is_some();
        // The token signed in to `base` with until now, if it isn't active.
        let earlier = if elsewhere && self.servers().contains(&base) {
            inner.stored(&token_key(&base))
        } else {
            None
        };
        if signed_in && !elsewhere {
            // The same user and workspace with another token would send its
            // waiting writes with that token; another workspace's wait for it.
            let before = self.offline_identity_on(&base);
            let same_token = inner.backend.current_token().as_deref() == Some(token);
            if !same_token && before.is_some() && before == offline_identity {
                let waiting = inner.backend.outbox_unsent().await;
                if waiting > 0 {
                    return Err(BackendError::PendingChanges(waiting));
                }
            }
        } else if let Some(earlier) = &earlier {
            // Signed in to `base` before, not active: the same rule for its
            // writes, which wait for its user.
            let before = self.offline_identity_on(&base);
            if earlier != token && before.is_some() && before == offline_identity {
                let waiting = inner
                    .backend
                    .outbox_unsent_of(&base, before.as_deref().unwrap_or_default())
                    .await;
                if waiting > 0 {
                    return Err(BackendError::PendingChanges(waiting));
                }
            }
        }

        // Kept first: with no keychain, nothing changes.
        let (key, id_key) = (token_key(&base), user_key(&base));
        // With a token per workspace (RFC 0031): its own, beside the others'.
        // Another user on the server: the workspaces were the one before's.
        let workspace = inner.backend.workspace(&user);
        if let Some(workspace) = &workspace {
            if inner
                .stored(&id_key)
                .is_some_and(|before| Some(before) != identity)
            {
                for gone in self.workspaces_on(&base) {
                    self.forget_workspace(&base, &gone);
                }
            }
            inner
                .store
                .set(&workspace_token_key(&base, workspace), token)
                .map_err(BackendError::Keychain)?;
            self.keep_workspace(&base, workspace, &user);
        }
        inner
            .store
            .set(&key, token)
            .map_err(BackendError::Keychain)?;
        if inner.backend.is_chosen_at_sign_in() {
            if let Err(e) = inner.store.set(SERVER_KEY, &base) {
                if !signed_in || (elsewhere && earlier.is_none()) {
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
        self.remember_scope(&base, scope.as_deref());
        if inner.multiple() {
            let added = {
                let mut servers = inner.servers.write();
                let added = !servers.contains(&base);
                if added {
                    servers.push(base.clone());
                }
                added
            };
            if added {
                inner.save_servers();
            }
        }

        if elsewhere {
            // Another server becomes the active one; the one that was stays
            // signed in (RFC 0022).
            self.activate(&base, token.to_owned(), true);
            *inner.user.write() = Some(user);
            inner.backend.use_identity(offline_identity.as_deref());
            if earlier.as_deref().is_some_and(|t| t != token) && !keep_reads {
                inner.backend.forget_offline_reads(&base).await;
            }
            if identity.is_some() {
                inner
                    .backend
                    .forget_offline_others(&self.signed_in_identities())
                    .await;
            }
            crate::info!(target: "elyra::auth", "signed in to {base}, beside the others");
            self.emit(None);
            self.tell(Change::Switched);
            self.emit_accounts(None, None);
            return Ok(());
        }

        inner.backend.choose_base(Some(&base));
        inner.backend.use_token(Some(token.to_owned()));
        *inner.user.write() = Some(user);
        inner.backend.use_identity(offline_identity.as_deref());
        if signed_in {
            // Another token's answers aren't this one's, unless each workspace
            // has its own copy; the outbox carries on as the new user.
            if !keep_reads {
                inner.backend.forget_offline_reads(&base).await;
            }
            inner.backend.resume_outbox();
            crate::info!(target: "elyra::auth", "switched, on {base}");
            self.emit(None);
            self.tell(Change::Switched);
        } else {
            if identity.is_some() {
                inner
                    .backend
                    .forget_offline_others(&self.signed_in_identities())
                    .await;
            }
            inner.backend.resume_outbox();
            crate::info!(target: "elyra::auth", "signed in to {base}");
            self.emit(None);
            self.tell(Change::SignedIn);
        }
        self.emit_accounts(None, None);
        Ok(())
    }

    // -- several servers (RFC 0022) ----------------------------------------

    /// The servers the user is signed in to, in the order they signed in,
    /// with who they are on each and which one is active. One at most
    /// without [`Backend::multiple_servers`]; none when signed out. Nothing
    /// is asked of a server: a user not read this run is `None`.
    pub fn accounts(&self) -> Vec<Account> {
        let inner = &self.inner;
        let active = self.active_server();
        let current = inner.user.read().clone();
        let parked = inner.parked.lock().clone();
        self.servers()
            .into_iter()
            .map(|server| {
                let is_active = active.as_deref() == Some(server.as_str());
                // Every workspace's on that server: the account's.
                let (pending, failed) = self
                    .offline_identities_on(&server)
                    .iter()
                    .map(|id| inner.backend.outbox_counts_of(&server, id))
                    .fold((0, 0), |(p, f), (p2, f2)| (p + p2, f + f2));
                let in_use = self.scope_on(&server);
                let id = self.identity_on(&server);
                let workspaces = self
                    .workspaces_on(&server)
                    .into_iter()
                    .map(|w| {
                        let (pending, failed) = id
                            .as_deref()
                            .map(|id| {
                                inner
                                    .backend
                                    .outbox_counts_of(&server, &scoped(id, Some(&w)))
                            })
                            .unwrap_or((0, 0));
                        Workspace {
                            user: self.workspace_user(&server, &w),
                            active: in_use.as_deref() == Some(w.as_str()),
                            pending,
                            failed,
                            id: w,
                        }
                    })
                    .collect();
                Account {
                    pending,
                    failed,
                    user_id: inner.stored(&user_key(&server)),
                    user: if is_active {
                        current.clone()
                    } else {
                        parked.get(&server).cloned()
                    },
                    active: is_active,
                    workspaces,
                    server,
                }
            })
            .collect()
    }

    /// [`accounts`](Self::accounts) and the active one, as `elyra:accounts`
    /// carries them.
    pub fn accounts_state(&self) -> AccountsState {
        AccountsState {
            accounts: self.accounts(),
            active: self.active_server(),
            signed_out_of: None,
            reason: None,
            signed_out_of_workspace: None,
        }
    }

    /// The server requests go to while signed in; `None` while signed out.
    pub fn active_server(&self) -> Option<String> {
        if self.is_signed_in() {
            self.inner.backend.base_url()
        } else {
            None
        }
    }

    /// Make `server`, one the user is signed in to, the active one — nothing
    /// is revoked or signed out of (RFC 0022). Requests go there with its
    /// token from now on; the broadcaster reconnects there, to the channels
    /// named from its user, and presence channels are joined there again;
    /// the abilities are read again, everything live that read the backend
    /// re-runs, and its outbox carries on. The offline copies stay, each
    /// server's its own. `elyra:auth` says `signedIn: true` once, with its
    /// user (`null` when it isn't known this run: `auth.user()` reads it) and
    /// `server`, and `elyra:accounts` the list. The active server already: a
    /// no-op.
    pub async fn switch_to(&self, server: &str) -> Result<(), BackendError> {
        let inner = &self.inner;
        let _changing = inner.changing.lock().await;
        let base = crate::backend::normalise(server);
        if !self.servers().contains(&base) {
            return Err(not_signed_in_to(&base));
        }
        if self.active_server().as_deref() == Some(base.as_str()) {
            return Ok(());
        }
        let Some(token) = inner.stored(&token_key(&base)) else {
            // Its token is gone from the keychain: not signed in to it.
            inner.servers.write().retain(|s| *s != base);
            inner.save_servers();
            self.emit_accounts(Some(&base), Some("expired"));
            return Err(not_signed_in_to(&base));
        };
        self.activate(&base, token, true);
        crate::info!(target: "elyra::auth", "switched to {base}");
        self.emit(None);
        self.tell(Change::Switched);
        self.emit_accounts(None, None);
        Ok(())
    }

    /// Sign out of `server` alone: its token is revoked there (best-effort)
    /// and forgotten, with its offline copy. The others stay signed in. The
    /// active server: the next one becomes active, as with
    /// [`switch_to`](Self::switch_to) — or, the last one, it's a sign-out as
    /// [`sign_out`](Self::sign_out) always was (`elyra:auth` says
    /// `signedIn: false`). `elyra:accounts` says which server was left.
    ///
    /// Refused with `PendingChanges` (kind `pending_changes`) while writes
    /// for it wait in the outbox, as `sign_out` is;
    /// [`sign_out_of_discarding`](Self::sign_out_of_discarding) drops them.
    pub async fn sign_out_of(&self, server: &str) -> Result<(), BackendError> {
        self.leave(server, false).await.map(|_| ())
    }

    /// [`sign_out_of`](Self::sign_out_of), dropping the writes that wait for
    /// `server` — how many went, also dispatched as an
    /// [`OutboxDropped`](crate::offline::OutboxDropped) when any did.
    pub async fn sign_out_of_discarding(&self, server: &str) -> Result<u32, BackendError> {
        self.leave(server, true).await
    }

    /// Sign out of every server: each token revoked on its own server and
    /// forgotten, the active one last. Refused with `PendingChanges` while
    /// writes for any of them wait (the count is all of them).
    pub async fn sign_out_all(&self) -> Result<(), BackendError> {
        self.leave_all(false).await.map(|_| ())
    }

    /// [`sign_out_all`](Self::sign_out_all), dropping every waiting write:
    /// how many went.
    pub async fn sign_out_all_discarding(&self) -> Result<u32, BackendError> {
        self.leave_all(true).await
    }

    // -- a token per workspace (RFC 0031) ----------------------------------

    /// Put `workspace`'s token on `server` in use — nothing is revoked or
    /// signed out of. On the active server it's a switch as
    /// [`sign_in_with_token`](Self::sign_in_with_token) with another
    /// workspace's token makes one: the broadcaster reconnects as the user
    /// there, the abilities are read again, everything live that read the
    /// backend re-runs, and each workspace keeps its own offline copy and
    /// outbox. On another server, that server becomes the active one, in
    /// that workspace. The workspace in use already: a no-op.
    pub async fn switch_workspace(
        &self,
        server: &str,
        workspace: &str,
    ) -> Result<(), BackendError> {
        let inner = &self.inner;
        let base = crate::backend::normalise(server);
        if !self.servers().contains(&base) {
            return Err(not_signed_in_to(&base));
        }
        if !self.workspaces_on(&base).iter().any(|w| w == workspace) {
            return Err(not_in_workspace(&base, workspace));
        }
        let token = inner.stored(&workspace_token_key(&base, workspace));
        let user = self.workspace_user(&base, workspace);
        let (Some(token), Some(user)) = (token, user) else {
            // Gone from the keychain: not signed in to it.
            self.forget_workspace(&base, workspace);
            self.emit_accounts_of(Some(&base), Some("expired"), Some(workspace));
            return Err(not_in_workspace(&base, workspace));
        };
        if self.active_server().as_deref() == Some(base.as_str())
            && inner.backend.current_token().as_deref() == Some(token.as_str())
        {
            return Ok(());
        }
        self.sign_in_with_token(&base, &token, user).await
    }

    /// Sign out of `workspace` on `server` alone: its token revoked there
    /// (with itself; best-effort) and forgotten. The one in use: the
    /// server's next workspace takes its place, as with
    /// [`switch_workspace`](Self::switch_workspace). The server's last:
    /// [`sign_out_of`](Self::sign_out_of) the server. `elyra:accounts` names
    /// the workspace left. Refused with `PendingChanges` while its writes
    /// wait in the outbox.
    pub async fn sign_out_of_workspace(
        &self,
        server: &str,
        workspace: &str,
    ) -> Result<(), BackendError> {
        let inner = &self.inner;
        let base = crate::backend::normalise(server);
        if !self.servers().contains(&base) {
            return Err(not_signed_in_to(&base));
        }
        let all = self.workspaces_on(&base);
        if !all.iter().any(|w| w == workspace) {
            return Err(not_in_workspace(&base, workspace));
        }
        if let Some(id) = self.identity_on(&base) {
            let waiting = inner
                .backend
                .outbox_unsent_of(&base, &scoped(&id, Some(workspace)))
                .await;
            if waiting > 0 {
                return Err(BackendError::PendingChanges(waiting));
            }
        }
        let next = all
            .iter()
            .filter(|w| *w != workspace)
            .find(|w| inner.stored(&workspace_token_key(&base, w)).is_some())
            .cloned();
        let Some(next) = next else {
            return self.sign_out_of(&base).await;
        };
        if self.scope_on(&base).as_deref() == Some(workspace) {
            if self.active_server().as_deref() == Some(base.as_str()) {
                self.switch_workspace(&base, &next).await?;
            } else {
                // Not the active server: the next one is what a switch
                // there brings back.
                let token = inner.stored(&workspace_token_key(&base, &next));
                if let Some(token) = token {
                    inner
                        .store
                        .set(&token_key(&base), &token)
                        .map_err(BackendError::Keychain)?;
                    self.remember_scope(&base, Some(&next));
                    match self.workspace_user(&base, &next) {
                        Some(user) => inner.parked.lock().insert(base.clone(), user),
                        None => inner.parked.lock().remove(&base),
                    };
                }
            }
        }
        if let Some(token) = inner.stored(&workspace_token_key(&base, workspace)) {
            let revoke = inner.backend.routes().revoke.clone();
            if let Err(e) = inner
                .backend
                .request_as(&base, &token, crate::http::Method::Delete, revoke)
                .send()
                .await
            {
                crate::debug!(target: "elyra::auth", "couldn't revoke the token for {base}#{workspace}: {e}");
            }
        }
        self.forget_workspace(&base, workspace);
        crate::info!(target: "elyra::auth", "signed out of workspace {workspace} on {base}");
        self.emit_accounts_of(Some(&base), Some("signed-out"), Some(workspace));
        Ok(())
    }

    /// A request to `path` with `workspace`'s token on `server`: to ask a
    /// workspace that isn't in use (what's unread there). A `401` signs
    /// nothing out — the app decides, with
    /// [`sign_out_of_workspace`](Self::sign_out_of_workspace) — and nothing
    /// is kept offline. No socket id.
    pub fn request_in(
        &self,
        server: &str,
        workspace: &str,
        method: crate::http::Method,
        path: impl Into<String>,
    ) -> Result<crate::backend::BackendRequest, BackendError> {
        let base = crate::backend::normalise(server);
        let token = self
            .inner
            .stored(&workspace_token_key(&base, workspace))
            .ok_or_else(|| not_in_workspace(&base, workspace))?;
        Ok(self.inner.backend.request_as(&base, &token, method, path))
    }

    /// The workspaces on `base` with a token of their own, in the order
    /// they were signed in to.
    fn workspaces_on(&self, base: &str) -> Vec<String> {
        self.inner
            .stored(&workspaces_key(base))
            .and_then(|list| serde_json::from_str(&list).ok())
            .unwrap_or_default()
    }

    fn save_workspaces_on(&self, base: &str, list: &[String]) {
        let inner = &self.inner;
        let outcome = if list.is_empty() {
            inner.store.delete(&workspaces_key(base))
        } else {
            inner.store.set(
                &workspaces_key(base),
                &serde_json::to_string(list).unwrap_or_default(),
            )
        };
        if let Err(e) = outcome {
            crate::debug!(target: "elyra::auth", "can't keep the workspaces on {base}: {e}");
        }
    }

    /// Who the user is in `workspace` on `base`, as kept.
    fn workspace_user(&self, base: &str, workspace: &str) -> Option<Value> {
        self.inner
            .stored(&workspace_user_key(base, workspace))
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    /// Keep `workspace` among `base`'s, with who the user is there (its
    /// token is kept by the caller).
    fn keep_workspace(&self, base: &str, workspace: &str, user: &Value) {
        let inner = &self.inner;
        if let Err(e) = inner.store.set(
            &workspace_user_key(base, workspace),
            &serde_json::to_string(user).unwrap_or_default(),
        ) {
            crate::debug!(target: "elyra::auth", "can't keep the user in {workspace}: {e}");
        }
        let mut list = self.workspaces_on(base);
        if !list.iter().any(|w| w == workspace) {
            list.push(workspace.to_owned());
            self.save_workspaces_on(base, &list);
        }
    }

    /// Forget `workspace` on `base`: its token, its user, its place.
    fn forget_workspace(&self, base: &str, workspace: &str) {
        let inner = &self.inner;
        for key in [
            workspace_token_key(base, workspace),
            workspace_user_key(base, workspace),
        ] {
            if let Err(e) = inner.store.delete(&key) {
                crate::debug!(target: "elyra::auth", "can't remove {key}: {e}");
            }
        }
        let mut list = self.workspaces_on(base);
        list.retain(|w| w != workspace);
        self.save_workspaces_on(base, &list);
    }

    /// Revoke every workspace's token on `base` but `except` (revoked by
    /// the caller), and forget them all: a sign-out of the server.
    async fn leave_workspaces(&self, base: &str, except: Option<&str>) {
        let inner = &self.inner;
        let revoke = inner.backend.routes().revoke.clone();
        for workspace in self.workspaces_on(base) {
            let token = inner.stored(&workspace_token_key(base, &workspace));
            if let Some(token) = token.filter(|t| Some(t.as_str()) != except) {
                if let Err(e) = inner
                    .backend
                    .request_as(base, &token, crate::http::Method::Delete, revoke.clone())
                    .send()
                    .await
                {
                    crate::debug!(target: "elyra::auth", "couldn't revoke the token for {base}#{workspace}: {e}");
                }
            }
            self.forget_workspace(base, &workspace);
        }
    }

    async fn leave_all(&self, discard: bool) -> Result<u32, BackendError> {
        let servers = self.servers();
        if servers.is_empty() {
            self.sign_out_now().await?;
            return Ok(0);
        }
        if !discard {
            let mut waiting = 0;
            for server in &servers {
                for id in self.offline_identities_on(server) {
                    waiting += self.inner.backend.outbox_unsent_of(server, &id).await;
                }
            }
            if waiting > 0 {
                return Err(BackendError::PendingChanges(waiting));
            }
        }
        let active = self.active_server();
        let mut dropped = 0;
        for server in servers.iter().filter(|s| Some(*s) != active.as_ref()) {
            dropped += self.leave(server, discard).await?;
        }
        if let Some(active) = active {
            dropped += self.leave(&active, discard).await?;
        }
        Ok(dropped)
    }

    /// Sign out of `server`; how many waiting writes were dropped.
    async fn leave(&self, server: &str, discard: bool) -> Result<u32, BackendError> {
        let inner = &self.inner;
        let base = crate::backend::normalise(server);
        let servers = self.servers();
        if !servers.contains(&base) {
            return Err(not_signed_in_to(&base));
        }
        let active = self.active_server().as_deref() == Some(base.as_str());
        // Every workspace's writes there, not only the one it's in now.
        let identities = self.offline_identities_on(&base);
        let mut waiting = 0;
        for id in &identities {
            waiting += inner.backend.outbox_unsent_of(&base, id).await;
        }
        if waiting > 0 && !discard {
            return Err(BackendError::PendingChanges(waiting));
        }
        let mut dropped = 0;
        if discard {
            for id in &identities {
                dropped += inner.backend.discard_outbox_of(&base, id).await;
            }
        }
        let next = servers
            .iter()
            .filter(|s| **s != base)
            .find_map(|s| inner.stored(&token_key(s)).map(|t| (s.clone(), t)));
        // The last one, or one server only: the sign-out there always was.
        if active && next.is_none() {
            self.sign_out_now().await?;
            return Ok(dropped);
        }
        let _changing = inner.changing.lock().await;
        let token = if active {
            inner.backend.current_token()
        } else {
            inner.stored(&token_key(&base))
        };
        // Another server first, so the UI never shows this one signed out:
        // then this one's token is revoked as an inactive server's.
        if let (true, Some((next, next_token))) = (active, next) {
            *inner.user.write() = None;
            self.activate(&next, next_token, false);
            crate::info!(target: "elyra::auth", "signed out of {base}; {next} is active");
            self.emit(None);
            self.tell(Change::Switched);
        }
        self.leave_workspaces(&base, token.as_deref()).await;
        if let Some(token) = token {
            let revoke = inner.backend.routes().revoke.clone();
            if let Err(e) = inner
                .backend
                .request_as(&base, &token, crate::http::Method::Delete, revoke)
                .send()
                .await
            {
                crate::debug!(target: "elyra::auth", "couldn't revoke the token for {base}: {e}");
            }
        }
        inner.backend.forget_offline_reads(&base).await;
        for key in [
            token_key(&base),
            user_key(&base),
            scope_key(&base),
            scopes_key(&base),
        ] {
            if let Err(e) = inner.store.delete(&key) {
                crate::debug!(target: "elyra::auth", "can't remove {key}: {e}");
            }
        }
        inner.parked.lock().remove(&base);
        inner.servers.write().retain(|s| *s != base);
        inner.save_servers();
        if !active {
            crate::info!(target: "elyra::auth", "signed out of {base}");
        }
        self.emit_accounts(Some(&base), Some("signed-out"));
        Ok(dropped)
    }

    /// The servers signed in to: the list with [`Backend::multiple_servers`],
    /// else the active one, when signed in.
    fn servers(&self) -> Vec<String> {
        let inner = &self.inner;
        if inner.multiple() {
            return inner.servers.read().clone();
        }
        self.active_server().into_iter().collect()
    }

    /// The user's id on `server`: the active one's as known now, another's
    /// as kept beside its token.
    fn identity_on(&self, server: &str) -> Option<String> {
        if self.active_server().as_deref() == Some(server) {
            self.current_identity()
        } else {
            self.inner.stored(&user_key(server))
        }
    }

    /// Each server signed in to, with the user's id there: whose offline
    /// copies stay.
    fn signed_in_identities(&self) -> Vec<(String, String)> {
        self.servers()
            .into_iter()
            .flat_map(|s| {
                let ids = self.offline_identities_on(&s);
                ids.into_iter().map(move |id| (s.clone(), id))
            })
            .collect()
    }

    /// The workspace the user is in on `server`: the user value's when it's
    /// the active one and known, else the one kept. `None` without
    /// `Backend::offline_scope_path`.
    fn scope_on(&self, server: &str) -> Option<String> {
        let inner = &self.inner;
        if !inner.backend.offline_is_scoped() {
            return None;
        }
        if self.active_server().as_deref() == Some(server) {
            let now = inner
                .user
                .read()
                .as_ref()
                .and_then(|user| inner.backend.offline_scope(user));
            if now.is_some() {
                return now;
            }
        }
        inner.stored(&scope_key(server))
    }

    /// Whose copy and outbox are in use on `server`: the user, in the
    /// workspace they're in there.
    fn offline_identity_on(&self, server: &str) -> Option<String> {
        self.identity_on(server)
            .map(|id| scoped(&id, self.scope_on(server).as_deref()))
    }

    /// Every copy and outbox of the user's on `server`: one per workspace
    /// they've been in, or just theirs without workspaces.
    fn offline_identities_on(&self, server: &str) -> Vec<String> {
        let Some(id) = self.identity_on(server) else {
            return Vec::new();
        };
        if !self.inner.backend.offline_is_scoped() {
            return vec![id];
        }
        let mut scopes = self.known_scopes(server);
        if let Some(now) = self.scope_on(server) {
            if !scopes.contains(&now) {
                scopes.push(now);
            }
        }
        if scopes.is_empty() {
            return vec![id];
        }
        scopes.iter().map(|s| scoped(&id, Some(s))).collect()
    }

    fn known_scopes(&self, server: &str) -> Vec<String> {
        self.inner
            .stored(&scopes_key(server))
            .and_then(|list| serde_json::from_str(&list).ok())
            .unwrap_or_default()
    }

    /// Keep `scope` as the workspace the user is in on `base`, and among
    /// those they've been in: what a restart, offline, reads its copy by.
    fn remember_scope(&self, base: &str, scope: Option<&str>) {
        let inner = &self.inner;
        if !inner.backend.offline_is_scoped() {
            return;
        }
        let Some(scope) = scope else {
            let _ = inner.store.delete(&scope_key(base));
            return;
        };
        if let Err(e) = inner.store.set(&scope_key(base), scope) {
            crate::debug!(target: "elyra::auth", "can't keep the workspace: {e}");
        }
        let mut known = self.known_scopes(base);
        if !known.iter().any(|k| k == scope) {
            known.push(scope.to_owned());
            if let Ok(list) = serde_json::to_string(&known) {
                let _ = inner.store.set(&scopes_key(base), &list);
            }
        }
    }

    /// The token for `server` — the active one's as requests carry it,
    /// another's from the keychain, when signed in to it. What the outbox
    /// sends that server's writes with.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn token_for(&self, server: &str) -> Option<String> {
        let inner = &self.inner;
        let base = crate::backend::normalise(server);
        if inner.backend.base_url().as_deref() == Some(base.as_str()) {
            return inner.backend.current_token();
        }
        if inner.multiple() && inner.servers.read().contains(&base) {
            return inner.stored(&token_key(&base));
        }
        None
    }

    /// Make `base` the active server, with `token`: the user signed in there
    /// (`park`: the one active until now stays known, for `accounts()`), its
    /// user as known, its offline copy and outbox, the connection state
    /// unknown again. The caller says so on `elyra:auth` and to the watchers.
    fn activate(&self, base: &str, token: String, park: bool) {
        let inner = &self.inner;
        let before = inner.backend.base_url();
        let user = inner.user.write().take();
        if let (true, Some(before), Some(user)) = (park, before, user) {
            inner.parked.lock().insert(before, user);
        }
        inner.backend.forget_connection();
        inner.backend.choose_base(Some(base));
        inner.backend.use_token(Some(token));
        *inner.user.write() = inner.parked.lock().remove(base);
        inner
            .backend
            .use_identity(self.offline_identity_on(base).as_deref());
        if let Err(e) = inner.store.set(SERVER_KEY, base) {
            crate::debug!(target: "elyra::auth", "can't keep the active server: {e}");
        }
        inner.backend.resume_outbox();
    }

    /// Something an account shows changed (its outbox's counts): the list
    /// goes to the windows again.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn accounts_changed(&self) {
        self.emit_accounts(None, None);
    }

    fn emit_accounts(&self, signed_out_of: Option<&str>, reason: Option<&str>) {
        self.emit_accounts_of(signed_out_of, reason, None);
    }

    /// …and the workspace left on `signed_out_of`, when it was one (RFC 0031).
    fn emit_accounts_of(
        &self,
        signed_out_of: Option<&str>,
        reason: Option<&str>,
        workspace: Option<&str>,
    ) {
        if let Some(bus) = &self.inner.bus {
            let mut state = self.accounts_state();
            state.signed_out_of = signed_out_of.map(str::to_owned);
            state.reason = reason.map(str::to_owned);
            state.signed_out_of_workspace = workspace.map(str::to_owned);
            let _ = bus.emit(ACCOUNTS_CHANNEL, &state);
        }
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
    ///
    /// With [`Backend::multiple_servers`] (RFC 0022), it's
    /// [`sign_out_of`](Self::sign_out_of) the active server: another one the
    /// user is signed in to becomes active, and only the last one leaves the
    /// user signed out. [`sign_out_all`](Self::sign_out_all) leaves them all.
    pub async fn sign_out(&self) -> Result<(), BackendError> {
        if let (true, Some(active)) = (self.inner.multiple(), self.active_server()) {
            return self.sign_out_of(&active).await;
        }
        let mut waiting = self.inner.backend.outbox_unsent().await;
        if let Some(base) = self
            .active_server()
            .filter(|_| self.inner.backend.offline_is_scoped())
        {
            // The other workspaces' writes on this server too.
            let now = self.offline_identity_on(&base);
            for id in self.offline_identities_on(&base) {
                if Some(&id) != now.as_ref() {
                    waiting += self.inner.backend.outbox_unsent_of(&base, &id).await;
                }
            }
        }
        if waiting > 0 {
            return Err(BackendError::PendingChanges(waiting));
        }
        self.sign_out_now().await
    }

    /// Sign out, dropping the writes that haven't been sent.
    pub async fn sign_out_discarding(&self) -> Result<(), BackendError> {
        if let (true, Some(active)) = (self.inner.multiple(), self.active_server()) {
            return self.sign_out_of_discarding(&active).await.map(|_| ());
        }
        self.inner.backend.discard_outbox().await;
        if let Some(base) = self
            .active_server()
            .filter(|_| self.inner.backend.offline_is_scoped())
        {
            for id in self.offline_identities_on(&base) {
                self.inner.backend.discard_outbox_of(&base, &id).await;
            }
        }
        self.sign_out_now().await
    }

    async fn sign_out_now(&self) -> Result<(), BackendError> {
        let inner = &self.inner;
        // Every other workspace's token there too (RFC 0031).
        if let Some(base) = inner.backend.base_url() {
            let current = inner.backend.current_token();
            self.leave_workspaces(&base, current.as_deref()).await;
        }
        if inner.backend.has_token() {
            let revoke = inner.backend.routes().revoke.clone();
            if let Err(e) = inner.backend.delete(&revoke).send().await {
                crate::debug!(target: "elyra::auth", "couldn't revoke the token: {e}");
            }
        }
        // The next person at this machine sees nothing of theirs: with
        // workspaces, no workspace's copy.
        if let Some(base) = inner
            .backend
            .base_url()
            .filter(|_| inner.backend.offline_is_scoped())
        {
            inner.backend.forget_offline_reads(&base).await;
            for key in [scope_key(&base), scopes_key(&base)] {
                let _ = inner.store.delete(&key);
            }
        }
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
            server: self.inner.backend.base_url(),
        }
    }

    async fn fetch_user(&self) -> Result<Value, BackendError> {
        let route = self.inner.backend.routes().user.clone();
        let user: Value = self.inner.backend.get(route).json().await?;
        *self.inner.user.write() = Some(user.clone());
        let inner = &self.inner;
        if let (Some(id), Some(user_key)) = (inner.backend.identity(&user), inner.user_key()) {
            let scope = inner.backend.offline_scope(&user);
            inner
                .backend
                .use_identity(Some(&scoped(&id, scope.as_deref())));
            if let Some(base) = inner.backend.base_url() {
                self.remember_scope(&base, scope.as_deref());
            }
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

    /// Drop the token everywhere, and say why. With other servers signed in
    /// to (RFC 0022), the next one becomes active instead: signed out of
    /// this one only, and `elyra:accounts` says so.
    fn forget(&self, reason: &str) {
        let inner = &self.inner;
        let current = inner.backend.base_url();
        // A token per workspace (RFC 0031): a refused one ends its workspace,
        // and the server's next carries on.
        if let (true, Some(base)) = (reason == "expired", current.as_deref()) {
            if self.forget_refused_workspace(base, reason) {
                return;
            }
        }
        if inner.multiple() {
            let next = current.as_ref().and_then(|current| {
                self.servers()
                    .into_iter()
                    .filter(|s| s != current)
                    .find_map(|s| inner.stored(&token_key(&s)).map(|t| (s, t)))
            });
            if let (Some(current), Some((next, token))) = (&current, next) {
                if let Err(e) = inner.store.delete(&token_key(current)) {
                    crate::debug!(target: "elyra::auth", "can't remove the stored sign-in: {e}");
                }
                inner.servers.write().retain(|s| s != current);
                inner.save_servers();
                *inner.user.write() = None;
                self.activate(&next, token, false);
                crate::info!(
                    target: "elyra::auth",
                    "signed out of {current} ({reason}); {next} is active"
                );
                self.emit(None);
                self.tell(Change::Switched);
                self.emit_accounts(Some(current), Some(reason));
                return;
            }
        }
        self.tell(Change::SignedOut);
        self.forget_quietly();
        crate::info!(target: "elyra::auth", "signed out ({reason})");
        self.emit(Some(reason));
        self.emit_accounts(current.as_deref(), Some(reason));
    }

    /// The workspace in use on `base` had its token refused: forget it, and
    /// put the server's next workspace in use. `false` when there's no next
    /// one (the server's sign-out follows), or no workspaces.
    fn forget_refused_workspace(&self, base: &str, reason: &str) -> bool {
        let inner = &self.inner;
        if !inner.backend.has_token_per_workspace() {
            return false;
        }
        let Some(gone) = self.scope_on(base) else {
            return false;
        };
        if !self.workspaces_on(base).contains(&gone) {
            return false;
        }
        let next = self
            .workspaces_on(base)
            .into_iter()
            .filter(|w| *w != gone)
            .find_map(|w| inner.stored(&workspace_token_key(base, &w)).map(|t| (w, t)));
        self.forget_workspace(base, &gone);
        let Some((next, token)) = next else {
            return false;
        };
        if let Err(e) = inner.store.set(&token_key(base), &token) {
            crate::debug!(target: "elyra::auth", "can't keep the workspace's token: {e}");
        }
        *inner.user.write() = self.workspace_user(base, &next);
        self.remember_scope(base, Some(&next));
        inner.backend.use_token(Some(token));
        inner
            .backend
            .use_identity(self.offline_identity_on(base).as_deref());
        inner.backend.resume_outbox();
        crate::info!(
            target: "elyra::auth",
            "signed out of workspace {gone} on {base} ({reason}); {next} is in use"
        );
        self.emit(None);
        self.tell(Change::Switched);
        self.emit_accounts_of(Some(base), Some(reason), Some(&gone));
        true
    }

    /// …and, for a backend chosen at sign-in, the server: the next start
    /// shows the sign-in screen, which asks for it again.
    fn forget_quietly(&self) {
        let inner = &self.inner;
        if let (true, Some(current)) = (inner.multiple(), inner.backend.base_url()) {
            inner.servers.write().retain(|s| *s != current);
            inner.save_servers();
        }
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
/// Asked about a server the user isn't signed in to.
fn not_signed_in_to(base: &str) -> BackendError {
    BackendError::Other {
        status: 0,
        message: format!("not signed in to {base}"),
    }
}

fn not_in_workspace(base: &str, workspace: &str) -> BackendError {
    BackendError::Other {
        status: 0,
        message: format!("not signed in to workspace {workspace} on {base}"),
    }
}

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
