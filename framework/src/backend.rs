//! A Laravel backend (RFC 0005): the API the desktop app shares with its web
//! half, called as the signed-in user, with Laravel's answers mapped to
//! Elyra's errors. Behind the `backend` feature.
//!
//! ```ignore
//! App::new().backend(Backend::new("https://crm.example.com"))
//!
//! #[command(live, can = "customers.view")]
//! async fn customers_index(ctx: Ctx, query: CustomerQuery) -> elyra::Result<Page<Customer>> {
//!     Ok(ctx.get::<Backend>().get("/api/customers").query(&query).json().await?)
//! }
//! ```
//!
//! A `422` becomes the same [`ValidationErrors`] a local command returns, so
//! a form shows the server's messages per field. A `401` forgets the token.
//! No answer at all is an `offline` error, which the UI can tell apart.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::http::{Http, HttpError, Method, Request, Response};
use crate::validation::ValidationErrors;

/// The Laravel API, as the signed-in user. Cheap to clone; clones share the
/// token.
#[derive(Clone)]
pub struct Backend {
    http: Http,
    base: String,
    token: Arc<RwLock<Option<String>>>,
    routes: Routes,
    /// Told when the server says the token is no longer good (a `401`).
    signed_out: Arc<RwLock<Option<SignedOut>>>,
    written: Arc<RwLock<Option<Written>>>,
    /// The Pusher connection's socket id while connected (RFC 0006): sent as
    /// `X-Socket-ID`, so Laravel's `->toOthers()` skips this app.
    socket_id: Arc<RwLock<Option<String>>>,
    broadcasting: Option<crate::broadcasting::Broadcasting>,
    follows: Vec<crate::broadcasting::Follow>,
    /// `.notifications()` (RFC 0008).
    notifications: Option<crate::notifications::Config>,
    /// `.offline()` (RFC 0007): set before the app starts.
    #[cfg(feature = "database")]
    offline_config: Option<crate::offline::Config>,
    /// …and the copy and the connection state, once the app has a database.
    #[cfg(feature = "database")]
    offline: Option<Arc<crate::offline::Offline>>,
}

/// What to do when the server says the token is no longer good.
type SignedOut = Arc<dyn Fn() + Send + Sync>;

/// What to do after a write changed a resource: re-run the live queries that
/// read it.
type Written = Arc<dyn Fn(&str) + Send + Sync>;

/// The routes `Auth` uses, relative to the base URL.
#[derive(Clone, Debug)]
pub(crate) struct Routes {
    pub token: String,
    pub revoke: String,
    pub user: String,
}

impl Backend {
    /// The backend at `base_url` (`https://crm.example.com`), with Laravel's
    /// documented routes for app tokens.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: Http::new(),
            base: base_url.into().trim_end_matches('/').to_owned(),
            token: Arc::new(RwLock::new(None)),
            // In `routes/api.php`, where Laravel prefixes them with `/api`.
            routes: Routes {
                token: "/api/sanctum/token".into(),
                revoke: "/api/sanctum/token".into(),
                user: "/api/user".into(),
            },
            signed_out: Arc::new(RwLock::new(None)),
            written: Arc::new(RwLock::new(None)),
            socket_id: Arc::new(RwLock::new(None)),
            broadcasting: None,
            follows: Vec::new(),
            notifications: None,
            #[cfg(feature = "database")]
            offline_config: None,
            #[cfg(feature = "database")]
            offline: None,
        }
    }

    /// Where a token is issued (`POST`); `/api/sanctum/token` by default.
    pub fn token_route(mut self, path: impl Into<String>) -> Self {
        self.routes.token = path.into();
        self
    }

    /// Where the current token is revoked (`DELETE`); `/api/sanctum/token`
    /// by default.
    pub fn revoke_route(mut self, path: impl Into<String>) -> Self {
        self.routes.revoke = path.into();
        self
    }

    /// Where the signed-in user is read (`GET`); `/api/user` by default.
    pub fn user_route(mut self, path: impl Into<String>) -> Self {
        self.routes.user = path.into();
        self
    }

    /// Listen to Laravel's broadcasting (RFC 0006) once the user is signed
    /// in: `Broadcasting::pusher(key)`, for Askr, Reverb or Pusher.
    pub fn broadcasting(mut self, broadcasting: crate::broadcasting::Broadcasting) -> Self {
        self.broadcasting = Some(broadcasting);
        self
    }

    /// Follow the private channel `name` (Laravel's `PrivateChannel('customers')`):
    /// each event on it is a `BackendEvent`, reaches the windows on
    /// `elyra:backend`, and re-runs the live queries that read `resource`.
    pub fn follow(mut self, name: &str, resource: impl Into<String>) -> Self {
        self.follows.push(crate::broadcasting::Follow::private(
            name,
            Some(resource.into()),
        ));
        self
    }

    /// [`follow`](Self::follow) each `(channel, resource)` — what
    /// `rata make:resource --backend` writes: `.follow_all(resources::follows())`.
    pub fn follow_all<N: AsRef<str>, R: Into<String>>(
        self,
        follows: impl IntoIterator<Item = (N, R)>,
    ) -> Self {
        follows.into_iter().fold(self, |backend, (name, resource)| {
            backend.follow(name.as_ref(), resource)
        })
    }

    /// [`follow`](Self::follow) a public channel (Laravel's `Channel('news')`).
    pub fn follow_public(mut self, name: &str, resource: impl Into<String>) -> Self {
        self.follows.push(crate::broadcasting::Follow::public(
            name,
            Some(resource.into()),
        ));
        self
    }

    /// Hear Laravel's notifications (RFC 0008): what `$user->notify(..)` sends
    /// through the `broadcast` channel, on the signed-in user's private
    /// channel (`App.Models.User.{id}`). Each is a
    /// [`NotificationReceived`](crate::notifications::NotificationReceived)
    /// and reaches the windows on `elyra:notification`. Needs
    /// [`broadcasting`](Self::broadcasting): the app won't start without it.
    pub fn notifications(mut self) -> Self {
        self.notifications.get_or_insert_with(Default::default);
        self
    }

    /// Where the `database` channel's list is read (`GET`);
    /// `/api/notifications` by default. Turns on [`notifications`](Self::notifications).
    pub fn notifications_route(mut self, path: impl Into<String>) -> Self {
        self.notifications
            .get_or_insert_with(Default::default)
            .route = path.into();
        self
    }

    /// The user's private channel, `{id}` filled with the signed-in user's
    /// id; Laravel's `App.Models.User.{id}` by default (a notifiable's
    /// `receivesBroadcastNotificationsOn`). Turns on
    /// [`notifications`](Self::notifications).
    pub fn notifications_channel(mut self, channel: impl Into<String>) -> Self {
        self.notifications
            .get_or_insert_with(Default::default)
            .channel = channel.into();
        self
    }

    /// How a notification is shown, when its data isn't `title`, `body` and
    /// `url`. Turns on [`notifications`](Self::notifications).
    pub fn present_notifications(
        mut self,
        present: impl Fn(&crate::notifications::Notification) -> crate::notifications::Presented
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.notifications
            .get_or_insert_with(Default::default)
            .presenter = Arc::new(present);
        self
    }

    pub(crate) fn notifications_config(&self) -> Option<&crate::notifications::Config> {
        self.notifications.as_ref()
    }

    /// Keep working without the server (RFC 0007): a `GET` that gets no answer
    /// answers from the last copy, kept in the app's database, which this
    /// needs (`App::database(..)`). The connection state is on
    /// `elyra:connection`; while offline, the health route is probed.
    #[cfg(feature = "database")]
    pub fn offline(mut self) -> Self {
        self.offline_config.get_or_insert_with(Default::default);
        self
    }

    /// What's probed while offline: Laravel's `/up` by default. Any answer,
    /// whatever its status, means the server is back.
    #[cfg(feature = "database")]
    pub fn health_route(mut self, path: impl Into<String>) -> Self {
        self.offline_config
            .get_or_insert_with(Default::default)
            .health = path.into();
        self
    }

    /// The offline copy's total size, in bytes (100 MiB by default); past it,
    /// the oldest answers go. An answer over 2 MiB is never kept.
    #[cfg(feature = "database")]
    pub fn offline_cache_limit(mut self, bytes: usize) -> Self {
        self.offline_config
            .get_or_insert_with(Default::default)
            .total = bytes;
        self
    }

    /// Whether the server answers, and since when it hasn't — `None` without
    /// [`offline`](Self::offline).
    #[cfg(feature = "database")]
    pub fn connection(&self) -> Option<crate::offline::ConnectionState> {
        self.offline.as_ref().map(|o| o.state())
    }

    #[cfg(feature = "database")]
    pub(crate) fn offline_config(&self) -> Option<&crate::offline::Config> {
        self.offline_config.as_ref()
    }

    #[cfg(feature = "database")]
    pub(crate) fn offline_handle(&self) -> Option<&Arc<crate::offline::Offline>> {
        self.offline.as_ref()
    }

    #[cfg(feature = "database")]
    pub(crate) fn with_offline(mut self, offline: Arc<crate::offline::Offline>) -> Self {
        self.offline = Some(offline);
        self
    }

    /// Forget the signed-in user's offline copy: they signed out.
    pub(crate) async fn forget_offline_copy(&self) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.forget_owner().await;
        }
    }

    /// Forget everyone else's offline copy: this user signed in.
    pub(crate) async fn forget_offline_others(&self) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.forget_others().await;
        }
    }

    /// Whose offline copy reads and writes use: the signed-in user's id.
    pub(crate) fn use_identity(&self, _identity: Option<&str>) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.use_identity(_identity);
        }
    }

    /// The broadcaster connected: the server is there.
    pub(crate) fn reached(&self) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.came_back();
        }
    }

    pub(crate) fn broadcasting_config(
        &self,
    ) -> Option<(
        crate::broadcasting::Broadcasting,
        Vec<crate::broadcasting::Follow>,
    )> {
        self.broadcasting.clone().map(|b| (b, self.follows.clone()))
    }

    pub(crate) fn use_socket_id(&self, socket_id: Option<String>) {
        *self.socket_id.write() = socket_id;
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub(crate) fn routes(&self) -> &Routes {
        &self.routes
    }

    /// This backend, answered by `fake` — for a test that uses a `Backend`
    /// without an app. In an app, `App::swap(Http::fake(..))` does it.
    pub fn with_fake_for_tests(self, fake: &crate::http::HttpFake) -> Self {
        self.with_http(Http::fake(fake.clone()))
    }

    /// Use the app's `Http` (so a test's `HttpFake` answers).
    pub(crate) fn with_http(mut self, http: Http) -> Self {
        self.http = http;
        self
    }

    /// The token requests carry. `Auth` sets it when the user signs in;
    /// a test may set one directly.
    pub fn use_token(&self, token: Option<String>) {
        *self.token.write() = token;
    }

    /// Whether a token is held.
    pub fn has_token(&self) -> bool {
        self.token.read().is_some()
    }

    /// Run `f` when the server says the token is no longer good.
    pub(crate) fn on_signed_out(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.signed_out.write() = Some(Arc::new(f));
    }

    /// Run `f` with a resource's key after a write to it succeeds.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn on_write(&self, f: impl Fn(&str) + Send + Sync + 'static) {
        *self.written.write() = Some(Arc::new(f));
    }

    pub fn get(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Get, path)
    }

    pub fn post(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Post, path)
    }

    pub fn put(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Put, path)
    }

    pub fn patch(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Patch, path)
    }

    pub fn delete(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Delete, path)
    }

    /// A request to `path` (`/api/customers`) on the backend.
    pub fn request(&self, method: Method, path: impl Into<String>) -> BackendRequest {
        let path = path.into();
        let mut request = self.http.request(method, format!("{}{}", self.base, path));
        if let Some(socket_id) = self.socket_id.read().as_deref() {
            request = request.header("X-Socket-ID", socket_id);
        }
        if let Some(token) = self.token.read().as_deref() {
            request = request.token(token);
        }
        BackendRequest {
            backend: self.clone(),
            method,
            path,
            request,
            no_offline: false,
            queue: false,
            replay: false,
        }
    }

    /// The outbox sends a kept write: as this user, with its key, and not
    /// kept again.
    #[cfg(feature = "database")]
    pub(crate) fn replay(&self, entry: &crate::offline::Entry) -> BackendRequest {
        let mut request = self.request(entry.method(), entry.target());
        if let Some(body) = entry.body() {
            request.request = request.request.raw_json(body.as_bytes().to_vec());
        }
        for (name, value) in entry.headers() {
            request.request = request.request.header(name.clone(), value.clone());
        }
        request.request = request.request.header("Idempotency-Key", entry.key());
        request.replay = true;
        request
    }

    /// Re-run the live queries that read `key`, as a write to it does.
    #[cfg(feature = "database")]
    pub(crate) fn touched(&self, key: &str) {
        let written = self.written.read().clone();
        if let Some(written) = written {
            written(key);
        }
    }

    /// Writes waiting in the outbox or refused, counted now.
    pub(crate) async fn outbox_unsent(&self) -> u32 {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            return offline.unsent().await;
        }
        0
    }

    /// Drop the signed-in user's outbox.
    pub(crate) async fn discard_outbox(&self) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.discard_all().await;
        }
    }

    /// Signed in again: the outbox carries on.
    pub(crate) fn resume_outbox(&self) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.resume();
        }
    }
}

/// A request to the backend; [`send`](Self::send) or [`json`](Self::json)
/// sends it.
pub struct BackendRequest {
    backend: Backend,
    method: Method,
    path: String,
    request: Request,
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    no_offline: bool,
    /// `.queue_offline()`: kept in the outbox when there's no answer.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    queue: bool,
    /// The outbox sending it: never kept again.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    replay: bool,
}

impl BackendRequest {
    /// Keep this answer out of the offline copy (RFC 0007): a one-time code,
    /// a report too big to keep. Offline, it's an `offline` error as before.
    pub fn no_offline(mut self) -> Self {
        self.no_offline = true;
        self
    }

    /// Keep this write for later when the server can't be reached (RFC 0007):
    /// it waits in the outbox, the answer is `BackendError::Queued` (kind
    /// `queued`), and it's sent — once, with its `Idempotency-Key` — when the
    /// server is back. While earlier writes wait, it waits behind them. Needs
    /// `Backend::offline()`; without it, this changes nothing.
    pub fn queue_offline(mut self) -> Self {
        self.queue = true;
        self
    }

    /// Only if the record hasn't changed since `updated_at` — the timestamp
    /// the user's copy had (Laravel's `updated_at`, as it sent it). Sent as
    /// `If-Unmodified-Since`; a server that checks it answers `412`, which is
    /// `BackendError::Conflict`, kind `conflict`. Without it, the last write
    /// wins, as in Laravel (RFC 0007).
    pub fn unless_changed_since(mut self, updated_at: impl AsRef<str>) -> Self {
        self.request = self
            .request
            .header("If-Unmodified-Since", updated_at.as_ref());
        self
    }

    pub fn query(mut self, query: &impl Serialize) -> Self {
        self.request = self.request.query(query);
        self
    }

    /// A JSON body.
    pub fn body(mut self, body: &impl Serialize) -> Self {
        self.request = self.request.json(body);
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.request = self.request.header(name, value);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.request = self.request.timeout(timeout);
        self
    }

    pub fn retry(mut self, times: u32, sleep: Duration) -> Self {
        self.request = self.request.retry(times, sleep);
        self
    }

    /// Send it: a success is the [`Response`], anything else a
    /// [`BackendError`].
    pub async fn send(self) -> Result<Response, BackendError> {
        let BackendRequest {
            backend,
            method,
            path,
            request,
            no_offline,
            queue,
            replay,
        } = self;
        #[cfg(not(feature = "database"))]
        let _ = (no_offline, queue, replay);
        if !path.starts_with('/') {
            return Err(BackendError::Http(HttpError::InvalidUrl(path)));
        }
        // Live queries: a read depends on its resource, and a live command's
        // re-run may not write (it would set itself off again).
        let key = resource_key(&path);
        let write = method != Method::Get;
        #[cfg(feature = "database")]
        if write {
            elyra_db::live::check_write(&key)
                .map_err(|e| BackendError::LiveWrite(e.to_string()))?;
        } else {
            // The resource and what it's under: a change to customers re-runs
            // what read `/api/customers/count` or `/api/customers/12/notes` too.
            for key in resource_keys(&path) {
                elyra_db::live::depends_on(&key);
            }
        }
        // Working offline (RFC 0007): a read is copied, and answered from the
        // copy when the server can't be reached — at once while it's known to
        // be offline, rather than waiting on the network each time.
        #[cfg(feature = "database")]
        let offline = backend.offline.clone();
        #[cfg(feature = "database")]
        let copy = match &offline {
            Some(o) if !write && !no_offline => {
                if let Some(top) = resource_keys(&path).pop() {
                    o.seen(top);
                }
                Some((o.clone(), request.full_url()))
            }
            _ => None,
        };
        #[cfg(feature = "database")]
        if let Some((o, url)) = &copy {
            if o.is_offline() && backend.has_token() {
                if let Some(kept) = o.lookup(url).await {
                    return Ok(kept);
                }
            }
        }
        // …and a write marked for it waits in the outbox when there's no
        // answer, with a key that makes sending it twice harmless. Known
        // offline, or behind earlier writes, it goes straight there.
        #[cfg(feature = "database")]
        let (request, entry) = match &offline {
            Some(o) if write && queue && !replay && backend.has_token() && o.owner().is_some() => {
                let key = crate::offline::idempotency_key();
                let request = request.header("Idempotency-Key", key.clone());
                let entry = crate::offline::Entry::new(method, &path, &request, key);
                if o.is_offline() || o.waiting().await {
                    return o.enqueue(entry).await;
                }
                (request, Some(entry))
            }
            _ => (request, None),
        };
        let sent = request.send().await;
        #[cfg(feature = "database")]
        if let Some(o) = &offline {
            match &sent {
                Err(HttpError::Unreachable(_)) => o.went_offline(),
                Ok(_) => o.came_back(),
                Err(_) => {}
            }
        }
        let response = match sent {
            Ok(response) => response,
            Err(HttpError::Unreachable(why)) => {
                #[cfg(feature = "database")]
                if let Some((o, url)) = &copy {
                    if backend.has_token() {
                        if let Some(kept) = o.lookup(url).await {
                            return Ok(kept);
                        }
                    }
                }
                #[cfg(feature = "database")]
                if let (Some(o), Some(entry)) = (&offline, entry) {
                    return o.enqueue(entry).await;
                }
                return Err(BackendError::Unreachable(why));
            }
            Err(other) => return Err(BackendError::Http(other)),
        };
        let outcome = answer(response);
        #[cfg(feature = "database")]
        if let (Ok(response), Some((o, url))) = (&outcome, &copy) {
            if backend.has_token() {
                o.store(url, response).await;
            }
        }
        if write && outcome.is_ok() {
            let written = backend.written.read().clone();
            if let Some(written) = written {
                written(&key);
            }
        }
        if matches!(
            outcome,
            Err(BackendError::Unauthenticated | BackendError::Expired)
        ) {
            // Only a token that was there can have expired: a `401` before
            // signing in isn't a sign-out.
            let had_token = backend.has_token();
            backend.use_token(None);
            let signed_out = backend.signed_out.read().clone();
            if let (true, Some(signed_out)) = (had_token, signed_out) {
                signed_out();
            }
        }
        outcome
    }

    /// Send it and decode a record: an API Resource's answer is wrapped in
    /// `{ "data": … }` (Laravel's default), a model returned as-is isn't —
    /// either works.
    pub async fn resource<T: DeserializeOwned>(self) -> Result<T, BackendError> {
        let value: Value = self.json().await?;
        let value = match value {
            Value::Object(mut map) if map.contains_key("data") => {
                map.remove("data").unwrap_or(Value::Null)
            }
            other => other,
        };
        serde_json::from_value(value)
            .map_err(|e| BackendError::Http(HttpError::Decode(e.to_string())))
    }

    /// Send it and decode a success as `T` (`204 No Content` as `null`).
    pub async fn json<T: DeserializeOwned>(self) -> Result<T, BackendError> {
        let response = self.send().await?;
        if response.bytes().is_empty() {
            return serde_json::from_value(Value::Null)
                .map_err(|e| BackendError::Http(HttpError::Decode(e.to_string())));
        }
        response.json().map_err(BackendError::Http)
    }
}

/// What a Laravel answer that isn't a success means.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    /// `422`: Laravel's validation bag, per field.
    #[error("{0}")]
    Validation(ValidationErrors),
    /// `401`: the token is gone (revoked, expired) — the user is signed out.
    #[error("signed out: sign in again")]
    Unauthenticated,
    /// `419`: a session-only check; for a token, the same as signed out.
    #[error("the session expired: sign in again")]
    Expired,
    /// `403`, with Laravel's message.
    #[error("{0}")]
    Forbidden(String),
    /// `404`.
    #[error("not found")]
    NotFound,
    /// `429`, and how many seconds to wait when the server said.
    #[error("too many requests{}", retry_after.map(|s| format!("; try again in {s} s")).unwrap_or_default())]
    TooManyRequests { retry_after: Option<u64> },
    /// `409` / `412`: the record changed since the user saw it — a write with
    /// [`unless_changed_since`](BackendRequest::unless_changed_since) and a
    /// server that checks it (RFC 0007).
    #[error("{message}")]
    Conflict { status: u16, message: String },
    /// `5xx`, without the error page's HTML.
    #[error("the server failed ({0})")]
    Server(u16),
    /// No answer: offline, DNS, TLS, or the timeout.
    #[error("can't reach the server: {0}")]
    Unreachable(String),
    /// Another status, with Laravel's message when it sent one.
    #[error("the server answered {status}: {message}")]
    Other { status: u16, message: String },
    /// The request couldn't be made, or the answer isn't the type asked for.
    #[error("{0}")]
    Http(HttpError),
    /// A live command's re-run tried to write — which would set it off again.
    #[error("{0}")]
    LiveWrite(String),
    /// Signed in, but there's no keychain to keep the token in (the token was
    /// revoked again).
    #[error("this system has no keychain to keep the sign-in in: {0}")]
    Keychain(String),
    /// No answer, so the write waits in the outbox (`.queue_offline()`, RFC
    /// 0007) — the outbox item's id. Not a failure: it's sent when the server
    /// is back. The code after the write runs only once it has happened.
    #[error("saved — it will be sent when the server can be reached")]
    Queued(i64),
    /// Signing out while writes haven't been sent: they'd be lost.
    /// `Auth::sign_out_discarding` signs out anyway.
    #[error("{0} change(s) haven't been sent yet")]
    PendingChanges(u32),
}

impl From<BackendError> for crate::Error {
    fn from(e: BackendError) -> Self {
        let message = e.to_string();
        match e {
            BackendError::Validation(bag) => bag.into(),
            BackendError::Unauthenticated | BackendError::Expired => {
                crate::Error::with_kind("unauthenticated", message)
            }
            // Not `forbidden`: that kind is the IPC guard's (a missing token or
            // capability). This is the server's policy saying no.
            BackendError::Forbidden(_) => crate::Error::with_kind("denied", message),
            BackendError::NotFound => crate::Error::with_kind("not-found", message),
            BackendError::TooManyRequests { .. } => {
                crate::Error::with_kind("too-many-requests", message)
            }
            BackendError::Server(_) => crate::Error::with_kind("server", message),
            BackendError::Unreachable(_) => crate::Error::with_kind("offline", message),
            BackendError::Queued(_) => crate::Error::with_kind("queued", message),
            BackendError::Conflict { .. } => crate::Error::with_kind("conflict", message),
            BackendError::PendingChanges(_) => crate::Error::with_kind("pending_changes", message),
            BackendError::Other { .. }
            | BackendError::Http(_)
            | BackendError::Keychain(_)
            | BackendError::LiveWrite(_) => crate::Error::Command(message),
        }
    }
}

/// The live-query key a request to `path` reads or writes: its resource, the
/// path without the query and without trailing ids — `/api/customers/12?x=1`
/// and `/api/customers` are both `backend:/api/customers`. That's what
/// Laravel's `apiResource` routes look like; anything else can use
/// `ctx.depends_on` / `ctx.invalidate`.
pub fn resource_key(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let mut segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    while segments.last().is_some_and(|s| is_id(s)) {
        segments.pop();
    }
    format!("backend:/{}", segments.join("/"))
}

/// The keys a read depends on: its resource, and each path it's under
/// (`/api/customers/12/notes` → `…/12/notes`, `…/12`, `/api/customers`, `/api`).
pub fn resource_keys(path: &str) -> Vec<String> {
    let key = resource_key(path);
    let mut segments: Vec<&str> = key
        .trim_start_matches("backend:/")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let mut keys = Vec::new();
    while !segments.is_empty() {
        keys.push(format!("backend:/{}", segments.join("/")));
        segments.pop();
    }
    keys
}

/// An id in a path: a number, a UUID, or a ULID.
fn is_id(segment: &str) -> bool {
    let number = segment.chars().all(|c| c.is_ascii_digit());
    let uuid = segment.len() == 36
        && segment.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    let ulid = segment.len() == 26 && segment.chars().all(|c| c.is_ascii_alphanumeric());
    number || uuid || ulid
}

/// Map a Laravel answer: a success stays the response.
fn answer(response: Response) -> Result<Response, BackendError> {
    let status = response.status();
    if response.ok() {
        return Ok(response);
    }
    let body: Option<Value> = response.json().ok();
    let message = body
        .as_ref()
        .and_then(|b| b["message"].as_str())
        .map(str::to_owned);
    Err(match status {
        422 => match body.as_ref().and_then(|b| b["errors"].as_object()) {
            Some(errors) => {
                let mut bag = ValidationErrors::new();
                for (field, messages) in errors {
                    for m in messages.as_array().into_iter().flatten() {
                        if let Some(m) = m.as_str() {
                            bag.add(field, m);
                        }
                    }
                }
                if bag.is_empty() {
                    bag.add(
                        "_",
                        message.unwrap_or_else(|| "The given data was invalid.".into()),
                    );
                }
                BackendError::Validation(bag)
            }
            None => BackendError::Other {
                status,
                message: message.unwrap_or_else(|| "The given data was invalid.".into()),
            },
        },
        401 => BackendError::Unauthenticated,
        403 => BackendError::Forbidden(
            message.unwrap_or_else(|| "This action is unauthorized.".into()),
        ),
        404 => BackendError::NotFound,
        409 | 412 => BackendError::Conflict {
            status,
            message: message.unwrap_or_else(|| "Someone changed this meanwhile.".into()),
        },
        419 => BackendError::Expired,
        429 => BackendError::TooManyRequests {
            retry_after: response
                .header("retry-after")
                .and_then(|s| s.trim().parse().ok()),
        },
        500..=599 => BackendError::Server(status),
        _ => BackendError::Other {
            status,
            message: message.unwrap_or_default(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resource_is_its_path_without_ids_or_query() {
        assert_eq!(resource_key("/api/customers"), "backend:/api/customers");
        assert_eq!(resource_key("/api/customers/12"), "backend:/api/customers");
        assert_eq!(
            resource_key("/api/customers?page=2&search=ada"),
            "backend:/api/customers"
        );
        assert_eq!(
            resource_key("/api/customers/12/notes"),
            "backend:/api/customers/12/notes"
        );
        assert_eq!(
            resource_key("/api/customers/12/notes/5"),
            "backend:/api/customers/12/notes"
        );
        assert_eq!(
            resource_key("/api/orders/9b2f5c1e-3d4a-4e5f-8a9b-0c1d2e3f4a5b"),
            "backend:/api/orders"
        );
        assert_eq!(
            resource_key("/api/orders/01JA2B3C4D5E6F7G8H9J0K1M2N"),
            "backend:/api/orders"
        );
        assert_eq!(
            resource_key("/api/customers/export"),
            "backend:/api/customers/export"
        );
        assert_eq!(resource_key("/api/user"), "backend:/api/user");
    }

    #[test]
    fn follow_all_follows_each_private_channel() {
        const FOLLOWS: &[(&str, &str)] = &[("customers", "/api/customers"), ("orders", "/x")];
        let backend = Backend::new("https://crm.test").follow_all(FOLLOWS.iter().copied());
        assert_eq!(
            backend.follows,
            [
                crate::broadcasting::Follow::private("customers", Some("/api/customers".into())),
                crate::broadcasting::Follow::private("orders", Some("/x".into())),
            ]
        );
        assert_eq!(backend.follows[0].wire, "private-customers");
        let none: [(&str, &str); 0] = [];
        assert!(Backend::new("https://crm.test")
            .follow_all(none)
            .follows
            .is_empty());
    }

    #[test]
    fn a_read_depends_on_what_its_under() {
        assert_eq!(
            resource_keys("/api/customers/12/notes?page=2"),
            [
                "backend:/api/customers/12/notes",
                "backend:/api/customers/12",
                "backend:/api/customers",
                "backend:/api"
            ]
        );
        assert_eq!(
            resource_keys("/api/customers/12"),
            ["backend:/api/customers", "backend:/api"]
        );
    }
}
