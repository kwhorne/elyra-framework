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
    /// The server, shared by every clone: fixed by [`Backend::new`], or chosen
    /// at sign-in ([`Backend::chosen_at_sign_in`]) and `None` until then.
    base: SharedBase,
    /// [`Backend::chosen_at_sign_in`]: the server is the user's to choose.
    chosen: bool,
    /// `.multiple_servers()` (RFC 0022): signed in to several at once.
    multiple: bool,
    /// `.user_id_path(..)`: where the user's id is in the user value.
    user_id_path: String,
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
    /// `.follow_user(..)`: channels named from the signed-in user.
    user_follows: Vec<crate::broadcasting::UserFollow>,
    /// `.notifications()` (RFC 0008).
    notifications: Option<crate::notifications::Config>,
    /// `.abilities()` (RFC 0010): where the user's abilities are read.
    abilities: Option<String>,
    /// `.presence(..)` (RFC 0015): the presence channels a window may join.
    presence: Vec<String>,
    /// `.listen(..)` (RFC 0023): the private channels a window may listen to.
    listen: Vec<String>,
    /// `.record_key(..)` (RFC 0017): the field an event's `model` names its
    /// record by, per resource key; `id` otherwise.
    record_keys: Vec<(String, String)>,
    /// `.assistant()` (RFC 0011): where the app's assistant reaches Claude.
    assistant: Option<String>,
    /// `.offline()` (RFC 0007): set before the app starts.
    #[cfg(feature = "database")]
    offline_config: Option<crate::offline::Config>,
    /// …and the copy and the connection state, once the app has a database.
    #[cfg(feature = "database")]
    offline: Option<Arc<crate::offline::Offline>>,
    /// Uploads and downloads while they run (RFC 0009), once in an app.
    transfers: Option<crate::transfers::Transfers>,
    /// The inspector's offline switch (debug builds): while on, nothing
    /// reaches the server.
    unplugged: crate::inspector::Unplugged,
}

/// The server's base URL, shared by a backend's clones and what reads it at
/// use time (the broadcaster, the offline copy's probe).
pub(crate) type SharedBase = Arc<RwLock<Option<String>>>;

/// The live-query key every read of the backend depends on: a switch to
/// another identity (`Auth::sign_in_with_token`) re-runs them all.
pub const ALL_KEY: &str = "backend:*";

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
        Self::with_base(Some(normalise(&base_url.into())), false)
    }

    /// A backend whose server the user chooses when they sign in — their
    /// company's own, typed on the sign-in screen. Until then there's no
    /// server: a request is `BackendError::NoServer` (kind
    /// `unauthenticated`). [`Auth::sign_in_with_token`](crate::auth::Auth::sign_in_with_token)
    /// chooses it, and it's remembered beside the token, so the user stays
    /// signed in to it across restarts; signing out forgets it. Every
    /// builder method works as for [`new`](Self::new).
    pub fn chosen_at_sign_in() -> Self {
        Self::with_base(None, true)
    }

    /// Stay signed in to several servers at once, one of them active (RFC
    /// 0022) — a company's own and a customer's, say. With
    /// [`chosen_at_sign_in`](Self::chosen_at_sign_in): a
    /// [`sign_in_with_token`](crate::auth::Auth::sign_in_with_token) to
    /// another server adds it and makes it the active one, without signing
    /// out of the others; [`Auth::switch_to`](crate::auth::Auth::switch_to)
    /// goes back. Requests, broadcasting, the offline copy and the outbox are
    /// the active server's; each server keeps its own token, user, copy and
    /// waiting writes. Without it, another server needs a sign-out first, as
    /// before. On a fixed server ([`new`](Self::new)) it changes nothing.
    pub fn multiple_servers(mut self) -> Self {
        self.multiple = true;
        self
    }

    /// Whether the user may be signed in to several servers at once.
    pub fn allows_multiple_servers(&self) -> bool {
        self.chosen && self.multiple
    }

    fn with_base(base: Option<String>, chosen: bool) -> Self {
        Self {
            http: Http::new(),
            base: Arc::new(RwLock::new(base)),
            chosen,
            multiple: false,
            user_id_path: "id".into(),
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
            user_follows: Vec::new(),
            notifications: None,
            abilities: None,
            presence: Vec::new(),
            listen: Vec::new(),
            record_keys: Vec::new(),
            assistant: None,
            #[cfg(feature = "database")]
            offline_config: None,
            #[cfg(feature = "database")]
            offline: None,
            transfers: None,
            unplugged: crate::inspector::Unplugged::default(),
        }
    }

    /// Where the user's id is in the user value, as a dotted path: `id` by
    /// default (Laravel's `/api/user`), `user.id` for a route that answers
    /// `{ "user": { "id": 7 }, "workspace": { … } }`. It names the user's
    /// channel (`{id}` in [`notifications_channel`](Self::notifications_channel)),
    /// the user among presence members, and whose offline copy it is.
    pub fn user_id_path(mut self, path: impl Into<String>) -> Self {
        self.user_id_path = path.into();
        self
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
        let from = self
            .broadcasting
            .as_ref()
            .and_then(|b| b.settings_path().map(str::to_owned));
        self.broadcasting = Some(match from {
            Some(path) if broadcasting.settings_path().is_none() => broadcasting.read_from(path),
            _ => broadcasting,
        });
        self
    }

    /// Listen to Laravel's broadcasting, with settings the server gives: for
    /// a server chosen at sign-in, whose Reverb or Pusher isn't known when
    /// the app is built (RFC 0021). After each sign-in (and at a start that
    /// restores one), `GET path` as the user answers
    /// `{ "key": "…", "host": "…", "port": 443, "scheme": "wss" }`, where
    /// `host`, `port` and `scheme` may be left out (the server's own host,
    /// as for [`Broadcasting::pusher`](crate::broadcasting::Broadcasting::pusher)).
    /// Nothing of it is kept: the key is public, and it's read again each
    /// time. A `404` or an answer without a key leaves broadcasting off,
    /// with a warning; everything else works. The follows and
    /// `Broadcasting::auth_route` are still the app's: pass
    /// `.broadcasting(Broadcasting::pusher("").auth_route(..))` for another
    /// auth route; its key is replaced.
    pub fn broadcasting_from(mut self, path: impl Into<String>) -> Self {
        let broadcasting = self
            .broadcasting
            .take()
            .unwrap_or_else(|| crate::broadcasting::Broadcasting::pusher(""));
        self.broadcasting = Some(broadcasting.read_from(path));
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

    /// Follow a private channel named from the signed-in user: `template`'s
    /// `{dotted.path}` placeholders are filled from the user value (a string
    /// or a number), at each connection, so a switch to another user or
    /// workspace follows the new names. Events on it are `BackendEvent`s on
    /// `name`, on `elyra:backend`, as for [`follow`](Self::follow), and re-run
    /// what read `resource`, when given. A placeholder the user doesn't have
    /// skips this follow, with a warning.
    ///
    /// ```ignore
    /// Backend::chosen_at_sign_in()
    ///     .follow_user("chat", "user.{user.id}", None)                         // private-user.7
    ///     .follow_user("workspace", "workspace.{workspace.id}", Some("/api/v1/issues"))
    /// ```
    pub fn follow_user(mut self, name: &str, template: &str, resource: Option<&str>) -> Self {
        self.user_follows.push(crate::broadcasting::UserFollow::new(
            name,
            template,
            resource.map(str::to_owned),
        ));
        self
    }

    /// [`follow`](Self::follow) a public channel (Laravel's `Channel('news')`).
    pub fn follow_public(mut self, name: &str, resource: impl Into<String>) -> Self {
        self.follows.push(crate::broadcasting::Follow::public(
            name,
            Some(resource.into()),
        ));
        self
    }

    /// The field Laravel's events name `resource`'s record by, in their
    /// `model` (RFC 0017): `id` unless set, `uuid` for a model whose route key
    /// is its UUID. An event naming a record re-runs what showed it and the
    /// lists; one that doesn't, everything that read the resource.
    pub fn record_key(mut self, resource: &str, field: impl Into<String>) -> Self {
        self.record_keys
            .push((resource_key(resource), field.into()));
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

    /// Whether a notification in the OS shows its title and body (the
    /// default), or only "New notification", for what a locked screen shows.
    /// Turns on [`notifications`](Self::notifications).
    pub fn notification_previews(mut self, previews: bool) -> Self {
        self.notifications
            .get_or_insert_with(Default::default)
            .previews = previews;
        self
    }

    /// Whether the unread count is on the app's icon and in the tray's
    /// tooltip (the default). Turns on [`notifications`](Self::notifications).
    pub fn notification_badge(mut self, badge: bool) -> Self {
        self.notifications
            .get_or_insert_with(Default::default)
            .badge = badge;
        self
    }

    /// Know what the signed-in user may do, by Laravel (RFC 0010): after
    /// sign-in the app reads `GET /api/abilities`, a list like
    /// `["customers.view", "invoices.*"]`, and keeps it in
    /// [`Abilities`](crate::abilities::Abilities). Signing out forgets it.
    pub fn abilities(mut self) -> Self {
        self.abilities
            .get_or_insert_with(|| "/api/abilities".into());
        self
    }

    /// Where the abilities are read: `/api/abilities` by default.
    pub fn abilities_route(mut self, path: impl Into<String>) -> Self {
        self.abilities = Some(path.into());
        self
    }

    pub(crate) fn abilities_config(&self) -> Option<&str> {
        self.abilities.as_deref()
    }

    /// Who else has a record open (RFC 0015): the Laravel presence channels
    /// a window may join, as names or namespaces (`customers.*`), without
    /// the `presence-` prefix. Laravel's `routes/channels.php` decides who
    /// may join and what each member shows; this decides which channels
    /// the app asks about at all. Needs [`broadcasting`](Self::broadcasting).
    ///
    /// ```ignore
    /// Backend::new(url)
    ///     .broadcasting(Broadcasting::pusher("app-key"))
    ///     .presence(["customers.*", "invoices.*"])
    /// ```
    pub fn presence<P: Into<String>>(mut self, patterns: impl IntoIterator<Item = P>) -> Self {
        self.presence.extend(
            patterns
                .into_iter()
                .map(Into::into)
                .map(|p| p.strip_prefix("presence-").map(str::to_owned).unwrap_or(p)),
        );
        self
    }

    pub(crate) fn presence_config(&self) -> &[String] {
        &self.presence
    }

    /// The private channels a window may listen to while it shows what
    /// they're about (RFC 0023), as names or namespaces (`task.*`), without
    /// the `private-` prefix: a run's output while its window is open, not
    /// every run's all the time. Laravel's `routes/channels.php` decides who
    /// may listen, through the broadcasting auth, as for
    /// [`follow`](Self::follow); this decides which channels the app asks
    /// about at all. The runtime's `listen(channel, handler)` listens.
    /// Needs [`broadcasting`](Self::broadcasting).
    ///
    /// ```ignore
    /// Backend::new(url)
    ///     .broadcasting(Broadcasting::pusher("app-key"))
    ///     .listen(["task.*"])
    /// ```
    pub fn listen<P: Into<String>>(mut self, patterns: impl IntoIterator<Item = P>) -> Self {
        self.listen.extend(
            patterns
                .into_iter()
                .map(Into::into)
                .map(|p| p.strip_prefix("private-").map(str::to_owned).unwrap_or(p)),
        );
        self
    }

    pub(crate) fn listen_config(&self) -> &[String] {
        &self.listen
    }

    /// The app's assistant (RFC 0011) reaches Claude through this server,
    /// with the user's token: `POST /api/elyra/assistant/v1/messages`, which
    /// `Route::elyraAssistant()` answers with the server's key. No key is in
    /// the app, and signed out the assistant can't be used.
    pub fn assistant(mut self) -> Self {
        self.assistant
            .get_or_insert_with(|| "/api/elyra/assistant".into());
        self
    }

    /// Where the assistant's requests go: `/api/elyra/assistant` by default,
    /// to which the Messages API's `/v1/messages` is added.
    pub fn assistant_route(mut self, path: impl Into<String>) -> Self {
        self.assistant = Some(path.into());
        self
    }

    #[cfg_attr(not(feature = "ai"), allow(dead_code))]
    pub(crate) fn assistant_config(&self) -> Option<&str> {
        self.assistant.as_deref()
    }

    /// The token requests carry now, if any.
    pub(crate) fn current_token(&self) -> Option<String> {
        self.token.read().clone()
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

    /// The most a signed-in user's queued uploads may keep, in bytes (500 MB
    /// by default). An upload marked `.queue_offline()` keeps a copy of its
    /// files while it waits; past this, it isn't kept, and it's the
    /// `offline` error instead (RFC 0009).
    #[cfg(feature = "database")]
    pub fn offline_upload_limit(mut self, bytes: u64) -> Self {
        self.offline_config
            .get_or_insert_with(Default::default)
            .upload_limit = bytes;
        self
    }

    /// Where queued uploads keep their copies: the app's data folder
    /// (`elyra-outbox-files`) unless set.
    #[cfg(feature = "database")]
    pub fn offline_upload_folder(mut self, folder: impl Into<std::path::PathBuf>) -> Self {
        self.offline_config
            .get_or_insert_with(Default::default)
            .upload_folder = Some(folder.into());
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

    /// Forget every offline copy of a read from `base`: another token's
    /// answers aren't this one's (a switch to another workspace), or the user
    /// signed out of it. Other servers' copies, and the outbox, stay.
    pub(crate) async fn forget_offline_reads(&self, _base: &str) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.forget_server(_base).await;
        }
    }

    /// Forget every offline copy but those of the users signed in now, each
    /// `(server, identity)`: someone signed in.
    pub(crate) async fn forget_offline_others(&self, _signed_in: &[(String, String)]) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            let owners: Vec<String> = _signed_in
                .iter()
                .map(|(base, id)| crate::offline::owner_of(base, id))
                .collect();
            offline.keep_only(&owners).await;
        }
    }

    /// Another server is active: whether the last one answered says nothing
    /// about this one.
    pub(crate) fn forget_connection(&self) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.forget_connection();
        }
    }

    /// Whose offline copy reads and writes use: the signed-in user's id.
    pub(crate) fn use_identity(&self, _identity: Option<&str>) {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            offline.use_identity(_identity);
        }
    }

    /// The inspector's offline switch, shared by every clone.
    pub(crate) fn unplugged(&self) -> &crate::inspector::Unplugged {
        &self.unplugged
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
        Vec<crate::broadcasting::UserFollow>,
    )> {
        self.broadcasting
            .clone()
            .map(|b| (b, self.follows.clone(), self.user_follows.clone()))
    }

    pub(crate) fn use_socket_id(&self, socket_id: Option<String>) {
        *self.socket_id.write() = socket_id;
    }

    /// The server's URL (`https://crm.example.com`, no trailing slash):
    /// always `Some` for [`new`](Self::new); for
    /// [`chosen_at_sign_in`](Self::chosen_at_sign_in), `None` until the user
    /// signs in, and again once they sign out.
    pub fn base_url(&self) -> Option<String> {
        self.base.read().clone()
    }

    /// Whether the server is chosen at sign-in rather than fixed.
    pub fn is_chosen_at_sign_in(&self) -> bool {
        self.chosen
    }

    /// Choose the server (`Auth`), or forget it (`None`) — only for a
    /// backend chosen at sign-in.
    pub(crate) fn choose_base(&self, base: Option<&str>) {
        if self.chosen {
            *self.base.write() = base.map(normalise);
        }
    }

    /// The base URL as every clone sees it, for what reads it at use time.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn shared_base(&self) -> SharedBase {
        self.base.clone()
    }

    /// The user's id in `user`, at [`user_id_path`](Self::user_id_path): a
    /// non-empty string or a number.
    pub(crate) fn user_id(&self, user: &Value) -> Option<String> {
        value_at(user, &self.user_id_path)
    }

    /// Who the user is, for the offline copy: their id, else their email.
    pub(crate) fn identity(&self, user: &Value) -> Option<String> {
        self.user_id(user)
            .or_else(|| user["email"].as_str().map(str::to_owned))
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

    /// The outbox sends a queued upload now.
    #[cfg(feature = "database")]
    pub(crate) fn resuming_upload(&self, outbox: i64) {
        if let Some(transfers) = &self.transfers {
            transfers.resuming(outbox);
        }
    }

    /// List uploads and downloads in `transfers` while they run.
    pub(crate) fn with_transfers(mut self, transfers: crate::transfers::Transfers) -> Self {
        self.transfers = Some(transfers);
        self
    }

    /// A download from a URL the API gave, such as Laravel's
    /// `Storage::temporaryUrl(..)` on S3: the URL carries its own signature,
    /// so the request carries no token, no socket id, and isn't kept for
    /// working offline. Only a download (`download_to`, `download_into`,
    /// `download`) or `send` makes sense on it.
    pub fn download_url(&self, url: impl Into<String>) -> BackendRequest {
        let url = url.into();
        BackendRequest {
            backend: self.clone(),
            method: Method::Get,
            request: self.http.get(&url),
            path: url,
            server: None,
            token: None,
            owner: None,
            no_server: false,
            no_offline: true,
            queue: false,
            replay: false,
            external: true,
            label: None,
            max_size: None,
            save: None,
            with_lists: false,
        }
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
        let base = self.base.read().clone();
        let no_server = base.is_none();
        let mut request = self.http.request(
            method,
            format!("{}{}", base.as_deref().unwrap_or_default(), path),
        );
        if let Some(socket_id) = self.socket_id.read().as_deref() {
            request = request.header("X-Socket-ID", socket_id);
        }
        let token = self.token.read().clone();
        if let Some(token) = token.as_deref() {
            request = request.token(token);
        }
        // Whose request it is, as of now: a switch to another server while
        // it's in flight doesn't make it the new one's (RFC 0022).
        #[cfg(feature = "database")]
        let owner = self.offline.as_ref().and_then(|o| o.owner());
        #[cfg(not(feature = "database"))]
        let owner = None;
        BackendRequest {
            backend: self.clone(),
            method,
            path,
            request,
            server: base,
            token,
            owner,
            no_server,
            no_offline: false,
            queue: false,
            replay: false,
            external: false,
            label: None,
            max_size: None,
            save: None,
            with_lists: false,
        }
    }

    /// A request to `path` on `base`, with `token`: a server the user is
    /// signed in to but that may not be the active one (RFC 0022) — to
    /// revoke its token, read its user, send its outbox. No socket id (the
    /// connection is the active server's), and nothing kept offline.
    pub(crate) fn request_as(
        &self,
        base: &str,
        token: &str,
        method: Method,
        path: impl Into<String>,
    ) -> BackendRequest {
        let path = path.into();
        let request = self
            .http
            .request(method, format!("{base}{path}"))
            .token(token);
        BackendRequest {
            backend: self.clone(),
            method,
            path,
            request,
            server: Some(base.to_owned()),
            token: Some(token.to_owned()),
            owner: None,
            no_server: false,
            no_offline: true,
            queue: false,
            replay: false,
            external: false,
            label: None,
            max_size: None,
            save: None,
            with_lists: false,
        }
    }

    /// The outbox sends a kept write: to the server it was kept for, with
    /// that server's token and its key, and not kept again.
    #[cfg(feature = "database")]
    pub(crate) fn replay(
        &self,
        entry: &crate::offline::Entry,
        base: &str,
        token: &str,
    ) -> BackendRequest {
        let mut request = self.request_as(base, token, entry.method(), entry.target());
        // The active server's connection: `->toOthers()` skips this app.
        if self.base_url().as_deref() == Some(base) {
            if let Some(socket_id) = self.socket_id.read().as_deref() {
                request.request = request.request.header("X-Socket-ID", socket_id);
            }
        }
        match crate::offline::upload_manifest(entry.body()) {
            // An upload: its parts again, the files from the copies.
            Some(parts) => {
                for part in parts {
                    request.request = match part {
                        crate::offline::KeptPart::Text { name, text } => {
                            request.request.field(name, text)
                        }
                        crate::offline::KeptPart::File {
                            name,
                            file,
                            file_name,
                            content_type,
                        } => {
                            let file_name = file_name.unwrap_or_else(|| "file".into());
                            let content_type =
                                content_type.unwrap_or_else(|| "application/octet-stream".into());
                            request
                                .request
                                .attach_as(name, file, file_name, content_type)
                        }
                    };
                }
            }
            None => {
                if let Some(body) = entry.body() {
                    request.request = request.request.raw_json(body.as_bytes().to_vec());
                }
            }
        }
        for (name, value) in entry.headers() {
            request.request = request.request.header(name.clone(), value.clone());
        }
        request.request = request.request.header("Idempotency-Key", entry.key());
        request.replay = true;
        request
    }

    /// A write to `path` succeeded: what read the records it names, and the
    /// lists they're in, re-run (RFC 0017).
    pub(crate) fn touched_path(&self, path: &str) {
        let written = self.written.read().clone();
        if let Some(written) = written {
            for key in write_keys(path) {
                written(&key);
            }
        }
    }

    /// The field an event's `model` names its record by, for `resource`:
    /// `id` unless [`record_key`](Self::record_key) says otherwise.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn record_field(&self, resource: &str) -> &str {
        let key = resource_key(resource);
        self.record_keys
            .iter()
            .find(|(r, _)| *r == key)
            .map_or("id", |(_, field)| field.as_str())
    }

    /// Writes waiting in the outbox or refused, counted now.
    pub(crate) async fn outbox_unsent(&self) -> u32 {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            return offline.unsent().await;
        }
        0
    }

    /// Writes of `identity` on `base` waiting or refused, whether that
    /// server is active or not (RFC 0022).
    pub(crate) async fn outbox_unsent_of(&self, _base: &str, _identity: &str) -> u32 {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            return offline
                .unsent_of(&crate::offline::owner_of(_base, _identity))
                .await;
        }
        0
    }

    /// Drop the signed-in user's outbox: how many writes went.
    pub(crate) async fn discard_outbox(&self) -> u32 {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            return offline.discard_all().await;
        }
        0
    }

    /// Drop the outbox of `identity` on `base`: how many writes went.
    pub(crate) async fn discard_outbox_of(&self, _base: &str, _identity: &str) -> u32 {
        #[cfg(feature = "database")]
        if let Some(offline) = &self.offline {
            return offline
                .discard_owner(&crate::offline::owner_of(_base, _identity), _base)
                .await;
        }
        0
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
    /// The server, the token and the offline owner it was made with: a
    /// switch while it's in flight doesn't change whose it is (RFC 0022).
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    server: Option<String>,
    token: Option<String>,
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    owner: Option<String>,
    /// Made before a server was chosen: it can't be sent.
    no_server: bool,
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    no_offline: bool,
    /// `.queue_offline()`: kept in the outbox when there's no answer.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    queue: bool,
    /// The outbox sending it: never kept again.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    replay: bool,
    /// `Backend::download_url`: an absolute URL, sent without the token.
    external: bool,
    /// What `transfers` calls it.
    label: Option<String>,
    /// `.max_size(..)`: an attached file larger than this isn't sent.
    max_size: Option<u64>,
    /// A download: where it's saved.
    save: Option<crate::http::SaveTo>,
    /// `.with_lists()` (RFC 0017): a record's read re-runs on every change
    /// to its resource.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    with_lists: bool,
}

impl BackendRequest {
    /// In a live query, re-run this read on every change to its resource, not
    /// only on changes to its record (RFC 0017): for a record's endpoint that
    /// shows other records too (customer 42 with "related customers").
    pub fn with_lists(mut self) -> Self {
        self.with_lists = true;
        self
    }

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

    /// A file, streamed from disk as a multipart part: what Laravel reads
    /// with `$request->file('name')` (RFC 0009). A `ChosenFile` from the
    /// frontend works, as does any path the app knows. With a file, a
    /// `PUT`/`PATCH`/`DELETE` is sent as a `POST` with `_method`, as PHP
    /// reads files only from a `POST`.
    pub fn attach(mut self, name: impl Into<String>, file: impl AsRef<std::path::Path>) -> Self {
        self.request = self.request.attach(name, file);
        self
    }

    /// A file, sent under another file name and type.
    pub fn attach_as(
        mut self,
        name: impl Into<String>,
        file: impl AsRef<std::path::Path>,
        file_name: impl Into<String>,
        content_type: impl Into<String>,
    ) -> Self {
        self.request = self.request.attach_as(name, file, file_name, content_type);
        self
    }

    /// Bytes, sent as a file part.
    pub fn attach_bytes(
        mut self,
        name: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
        file_name: impl Into<String>,
        content_type: impl Into<String>,
    ) -> Self {
        self.request = self
            .request
            .attach_bytes(name, bytes, file_name, content_type);
        self
    }

    /// A text field beside the files.
    pub fn field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.request = self.request.field(name, value);
        self
    }

    /// A struct's fields as text fields beside the files: `None` left out,
    /// a list as `key[]`, `true` as `1`, as Laravel reads a form.
    pub fn fields(mut self, fields: &impl Serialize) -> Self {
        self.request = self.request.fields(fields);
        self
    }

    /// What `transfers` calls this upload or download; otherwise the file's
    /// name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Refuse an attached file larger than `bytes` before sending it, with
    /// the validation error on its field. The server's rule is what counts;
    /// this saves sending a file it would refuse.
    pub fn max_size(mut self, bytes: u64) -> Self {
        self.max_size = Some(bytes);
        self
    }

    /// Safe to repeat: an `Idempotency-Key` (which the server's middleware
    /// honours), so `.retry(..)` retries a `POST` or `PATCH` too.
    pub fn idempotent(mut self) -> Self {
        let key = crate::security::random_token()[..32].to_owned();
        self.request = self
            .request
            .header("Idempotency-Key", key)
            .retry_any_method();
        self
    }

    /// Stream the answer into the file the user chose in a save dialog,
    /// replacing it. The file is marked as downloaded (quarantine on macOS,
    /// the Mark of the Web on Windows). Returns where it was saved.
    pub async fn download_to(
        mut self,
        file: &crate::files::ChosenFile,
    ) -> Result<std::path::PathBuf, BackendError> {
        if file.kind() != crate::files::FileChoice::Save {
            return Err(BackendError::Http(HttpError::File(format!(
                "{} wasn't chosen to be saved to",
                file.name()
            ))));
        }
        self.save = Some(crate::http::SaveTo::File(file.path().to_owned()));
        self.download_now().await
    }

    /// Stream the answer into a new file in the folder the user chose, under
    /// the server's name, cleaned and numbered when taken.
    pub async fn download_into(
        mut self,
        folder: &crate::files::ChosenFile,
    ) -> Result<std::path::PathBuf, BackendError> {
        if folder.kind() != crate::files::FileChoice::Folder {
            return Err(BackendError::Http(HttpError::File(format!(
                "{} isn't a folder the user chose",
                folder.name()
            ))));
        }
        self.save = Some(crate::http::SaveTo::Into(folder.path().to_owned()));
        self.download_now().await
    }

    /// Stream the answer into the user's Downloads folder, under the
    /// server's name, cleaned and numbered when taken.
    pub async fn download(mut self) -> Result<std::path::PathBuf, BackendError> {
        let folder = dirs::download_dir().ok_or_else(|| {
            BackendError::Http(HttpError::File(
                "this system has no Downloads folder".into(),
            ))
        })?;
        self.save = Some(crate::http::SaveTo::Into(folder));
        self.download_now().await
    }

    async fn download_now(mut self) -> Result<std::path::PathBuf, BackendError> {
        // A file isn't an answer to keep for working offline.
        self.no_offline = true;
        let response = self.send().await?;
        response
            .saved_to()
            .map(|p| p.to_owned())
            .ok_or_else(|| BackendError::Http(HttpError::File("the answer wasn't saved".into())))
    }

    /// Send it: a success is the [`Response`], anything else a
    /// [`BackendError`].
    pub async fn send(self) -> Result<Response, BackendError> {
        let BackendRequest {
            backend,
            method,
            path,
            request,
            server,
            token,
            owner,
            no_server,
            no_offline,
            queue,
            replay,
            external,
            label,
            max_size,
            save,
            with_lists,
        } = self;
        #[cfg(not(feature = "database"))]
        let _ = (no_offline, queue, replay, with_lists, &server, &owner);
        if external {
            return send_external(&backend, request, save, label).await;
        }
        // A server chosen at sign-in, before anyone has: there's nowhere to
        // send it. Not a sign-out: nobody was signed in.
        if no_server {
            return Err(BackendError::NoServer);
        }
        if !path.starts_with('/') {
            return Err(BackendError::Http(HttpError::InvalidUrl(path)));
        }
        // Files (RFC 0009): too large to send, refused here; PHP's files only
        // come with a `POST`.
        let attachments = request.attachments();
        if let Some(max) = max_size {
            too_large(&attachments, max)?;
        }
        let request = request.spoof_method();
        // Live queries: a read depends on its resource, and a live command's
        // re-run may not write (it would set itself off again).
        let write = method != Method::Get;
        #[cfg(feature = "database")]
        if write {
            elyra_db::live::check_write(&resource_key(&path))
                .map_err(|e| BackendError::LiveWrite(e.to_string()))?;
        } else {
            // The resource and what it's under, and the record it read or
            // the resource's lists (RFC 0017): a change to customer 40
            // re-runs the lists and customer 40, not customer 42.
            for key in read_keys(&path, with_lists) {
                elyra_db::live::depends_on(&key);
            }
            // …and the backend as a whole: a switch of identity re-runs it.
            elyra_db::live::depends_on(ALL_KEY);
        }
        // Working offline (RFC 0007): a read is copied, and answered from the
        // copy when the server can't be reached — at once while it's known to
        // be offline, rather than waiting on the network each time.
        #[cfg(feature = "database")]
        let offline = backend.offline.clone();
        // Made as the active server's user: the copy and the connection
        // state are theirs. A request to another server (`request_as`), or
        // one that answers after a switch, touches neither.
        #[cfg(feature = "database")]
        let current = server.is_some() && server == backend.base_url();
        #[cfg(feature = "database")]
        let copy = match (&offline, &owner, &server) {
            (Some(o), Some(owner), Some(base))
                if !write && !no_offline && save.is_none() && token.is_some() =>
            {
                if let Some(top) = resource_keys(&path).pop() {
                    o.seen(top);
                }
                Some((o.clone(), owner.clone(), base.clone(), request.full_url()))
            }
            _ => None,
        };
        #[cfg(feature = "database")]
        if let Some((o, owner, _, url)) = &copy {
            if o.is_offline() && current {
                if let Some(kept) = o.lookup(owner, url).await {
                    return Ok(kept);
                }
            }
        }
        // …and a write marked for it waits in the outbox when there's no
        // answer, with a key that makes sending it twice harmless. Known
        // offline, or behind earlier writes, it goes straight there. It's
        // kept for the user and server it was made as.
        #[cfg(feature = "database")]
        let (request, entry) = match (&offline, &owner) {
            (Some(o), Some(owner)) if write && queue && !replay && token.is_some() => {
                let key = crate::offline::idempotency_key();
                let request = request.header("Idempotency-Key", key.clone());
                // As it will be sent: a `POST` with `_method` when it has files.
                let entry = crate::offline::Entry::new(request.method(), &path, &request, key)
                    .for_owner(owner.clone(), server.clone());
                // An upload keeps its parts, copied only if it has to wait.
                let parts = request.parts().to_vec();
                if (o.is_offline() && current) || o.waiting(owner).await {
                    return keep_for_later(
                        &backend,
                        o,
                        entry,
                        &parts,
                        label,
                        &path,
                        &attachments,
                        None,
                    )
                    .await;
                }
                (request, Some((entry, parts)))
            }
            _ => (request, None),
        };
        #[cfg(not(feature = "database"))]
        let entry: Option<()> = None;
        // An upload or a download is listed in `transfers` while it runs.
        let direction = if save.is_some() {
            Some(crate::transfers::Direction::Download)
        } else if !attachments.is_empty() {
            Some(crate::transfers::Direction::Upload)
        } else {
            None
        };
        #[cfg(feature = "database")]
        let kept_label = label.clone();
        let (request, transfer) = track(&backend, request, direction, label, &path, &attachments);
        let sent = run(&backend, request, save).await;
        let transfer = match (transfer, &sent) {
            // It may wait in the outbox instead.
            (Some(transfer), Err(HttpError::Unreachable(_))) if entry.is_some() => Some(transfer),
            (Some(transfer), Err(_)) => {
                transfer.finish(&sent);
                None
            }
            (transfer, _) => transfer,
        };
        #[cfg(feature = "database")]
        if let (Some(o), true) = (&offline, current) {
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
                if let Some((o, owner, _, url)) = &copy {
                    if let Some(kept) = o.lookup(owner, url).await {
                        return Ok(kept);
                    }
                }
                #[cfg(feature = "database")]
                if let (Some(o), Some((entry, parts))) = (&offline, entry) {
                    return keep_for_later(
                        &backend,
                        o,
                        entry,
                        &parts,
                        kept_label,
                        &path,
                        &attachments,
                        transfer,
                    )
                    .await;
                }
                if let Some(transfer) = transfer {
                    transfer.finish(&Err::<(), _>(HttpError::Unreachable(why.clone())));
                }
                return Err(BackendError::Unreachable(why));
            }
            Err(HttpError::Cancelled) => return Err(BackendError::Cancelled),
            Err(other) => return Err(BackendError::Http(other)),
        };
        let outcome = if response.status() == 413 && !attachments.is_empty() {
            Err(BackendError::Validation(too_large_for_the_server(
                &attachments,
            )))
        } else {
            answer(response)
        };
        if let Some(transfer) = transfer {
            match &outcome {
                Ok(response) => {
                    if let Some(name) = response.saved_to().and_then(|p| p.file_name()) {
                        transfer.rename(name.to_string_lossy().into_owned());
                    }
                    transfer.finish(&Ok::<(), HttpError>(()));
                }
                Err(e) => transfer.refused(e.to_string()),
            }
        }
        #[cfg(feature = "database")]
        if let (Ok(response), Some((o, owner, base, url))) = (&outcome, &copy) {
            o.store(owner, base, url, response).await;
        }
        if write && outcome.is_ok() {
            backend.touched_path(&path);
        }
        if matches!(
            outcome,
            Err(BackendError::Unauthenticated | BackendError::Expired)
        ) {
            // Only a token that was there can have expired: a `401` before
            // signing in isn't a sign-out. Nor is one for a token that isn't
            // the active one any more — another server's (RFC 0022), or one
            // a switch replaced while this was in flight.
            if token.is_some() && backend.current_token() == token {
                backend.use_token(None);
                let signed_out = backend.signed_out.read().clone();
                if let Some(signed_out) = signed_out {
                    signed_out();
                }
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

/// A write with no answer waits in the outbox; an upload with copies of its
/// files. A queued upload is listed in `transfers` as `queued`.
#[cfg(feature = "database")]
#[allow(clippy::too_many_arguments)]
async fn keep_for_later(
    backend: &Backend,
    offline: &Arc<crate::offline::Offline>,
    entry: crate::offline::Entry,
    parts: &[crate::http::Part],
    label: Option<String>,
    path: &str,
    attachments: &[(String, Option<String>, Option<u64>)],
    transfer: Option<crate::transfers::Handle>,
) -> Result<Response, BackendError> {
    if attachments.is_empty() {
        return offline.enqueue(entry).await;
    }
    let queued = offline.enqueue_upload(entry, parts).await;
    match (&queued, transfer, &backend.transfers) {
        (Err(BackendError::Queued(id)), Some(transfer), _) => transfer.queued(*id),
        (Err(BackendError::Queued(id)), None, Some(transfers)) => {
            let total = attachments.iter().map(|(_, _, size)| *size).sum();
            transfers.queued(transfer_name(label, attachments, path), total, *id);
        }
        (Err(e), Some(transfer), _) => transfer.refused(e.to_string()),
        _ => {}
    }
    queued
}

/// What `transfers` calls a transfer: its label, else the (first) file's
/// name, else the URL's last segment.
fn transfer_name(
    label: Option<String>,
    attachments: &[(String, Option<String>, Option<u64>)],
    path: &str,
) -> String {
    label
        .or_else(|| attachments.iter().find_map(|(_, name, _)| name.clone()))
        .unwrap_or_else(|| {
            let bare = path.split(['?', '#']).next().unwrap_or(path);
            bare.rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(bare)
                .to_owned()
        })
}

/// Start listing an upload or download in `transfers`, when the backend is
/// in an app that has them.
fn track(
    backend: &Backend,
    request: Request,
    direction: Option<crate::transfers::Direction>,
    label: Option<String>,
    path: &str,
    attachments: &[(String, Option<String>, Option<u64>)],
) -> (Request, Option<crate::transfers::Handle>) {
    let (Some(direction), Some(transfers)) = (direction, &backend.transfers) else {
        return (request, None);
    };
    let name = transfer_name(label, attachments, path);
    let total = (direction == crate::transfers::Direction::Upload)
        .then(|| {
            attachments
                .iter()
                .map(|(_, _, size)| *size)
                .sum::<Option<u64>>()
        })
        .flatten();
    let handle = transfers.start(direction, name, total);
    let request = request
        .on_progress(handle.reporter())
        .cancel_with(handle.cancel());
    (request, Some(handle))
}

/// Out to the network — or, while the inspector has it unplugged, the
/// answer a network with no server gives.
async fn run(
    backend: &Backend,
    request: Request,
    save: Option<crate::http::SaveTo>,
) -> Result<Response, HttpError> {
    if backend.unplugged.is_on() {
        return Err(HttpError::Unreachable(
            crate::inspector::Unplugged::REASON.into(),
        ));
    }
    request.run(save).await
}

/// `download_url`: no token, no live queries, nothing kept offline.
async fn send_external(
    backend: &Backend,
    request: Request,
    save: Option<crate::http::SaveTo>,
    label: Option<String>,
) -> Result<Response, BackendError> {
    let direction = save.as_ref().map(|_| crate::transfers::Direction::Download);
    let (request, transfer) = track(backend, request, direction, label, "download", &[]);
    let sent = run(backend, request, save).await;
    let response = match sent {
        Ok(response) => response,
        Err(e) => {
            if let Some(transfer) = transfer {
                transfer.finish(&Err::<(), _>(e.clone()));
            }
            return Err(match e {
                HttpError::Unreachable(why) => BackendError::Unreachable(why),
                HttpError::Cancelled => BackendError::Cancelled,
                other => BackendError::Http(other),
            });
        }
    };
    let outcome = answer(response);
    if let Some(transfer) = transfer {
        match &outcome {
            Ok(response) => {
                if let Some(name) = response.saved_to().and_then(|p| p.file_name()) {
                    transfer.rename(name.to_string_lossy().into_owned());
                }
                transfer.finish(&Ok::<(), HttpError>(()));
            }
            Err(e) => transfer.refused(e.to_string()),
        }
    }
    outcome
}

/// `.max_size(..)`: each attached file over it, as Laravel's `max` rule
/// would say.
fn too_large(
    attachments: &[(String, Option<String>, Option<u64>)],
    max: u64,
) -> Result<(), BackendError> {
    let mut bag = ValidationErrors::new();
    for (field, _, size) in attachments {
        if size.is_some_and(|size| size > max) {
            bag.add(
                field,
                format!("The file may not be larger than {}.", human_size(max)),
            );
        }
    }
    if bag.is_empty() {
        Ok(())
    } else {
        Err(BackendError::Validation(bag))
    }
}

/// A `413`: the web server (nginx's `client_max_body_size`) or PHP
/// (`post_max_size`) refused the body before Laravel saw it. The message
/// goes on the attached files' fields, where a form shows it.
fn too_large_for_the_server(
    attachments: &[(String, Option<String>, Option<u64>)],
) -> ValidationErrors {
    let mut bag = ValidationErrors::new();
    for (field, _, _) in attachments {
        if !bag.has(field) {
            bag.add(field, "The file is too large for the server.");
        }
    }
    bag
}

/// `20 MB`, `512 KB`, `100 bytes`.
fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    let (unit, name) = match bytes {
        b if b >= GB => (GB, "GB"),
        b if b >= MB => (MB, "MB"),
        b if b >= KB => (KB, "KB"),
        _ => return format!("{bytes} bytes"),
    };
    let value = bytes as f64 / unit as f64;
    if value.fract() == 0.0 {
        format!("{value:.0} {name}")
    } else {
        format!("{value:.1} {name}")
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
    /// The upload or download was cancelled (`Transfers::cancel`).
    #[error("cancelled")]
    Cancelled,
    /// A backend chosen at sign-in, before the user has: no server to ask.
    /// Kind `unauthenticated`, as for a user who isn't signed in.
    #[error("no server yet: sign in first")]
    NoServer,
}

impl From<BackendError> for crate::Error {
    fn from(e: BackendError) -> Self {
        let message = e.to_string();
        match e {
            BackendError::Validation(bag) => bag.into(),
            BackendError::Unauthenticated | BackendError::Expired | BackendError::NoServer => {
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
            BackendError::Cancelled => crate::Error::with_kind("cancelled", message),
            BackendError::Other { .. }
            | BackendError::Http(_)
            | BackendError::Keychain(_)
            | BackendError::LiveWrite(_) => crate::Error::Command(message),
        }
    }
}

/// `https://crm.example.com/` → `https://crm.example.com`.
pub(crate) fn normalise(base: &str) -> String {
    base.trim().trim_end_matches('/').to_owned()
}

/// The value at `path` (`user.id`) in `value`, as text: a non-empty string or
/// a number. Anything else, or nothing there, is `None`.
pub(crate) fn value_at(value: &Value, path: &str) -> Option<String> {
    let mut at = value;
    for segment in path.split('.') {
        at = match at {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    match at {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// `template` with each `{dotted.path}` filled from `value`
/// (`workspace.{workspace.id}` → `workspace.3`); the first placeholder that
/// doesn't resolve is the error.
pub(crate) fn fill(template: &str, value: &Value) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("an unclosed `{{` in `{template}`"))?;
        let path = after[..close].trim();
        let filled = value_at(value, path).ok_or_else(|| path.to_owned())?;
        out.push_str(&filled);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
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

/// The key of `path`'s resource's lists (RFC 0017): `backend:list:/api/customers`
/// for `/api/customers?page=2` or `/api/customers/12`. Every write to the
/// resource invalidates it; depend on it for a command that wants every change.
pub fn list_key(path: &str) -> String {
    let resource = resource_key(path);
    format!("backend:list:/{}", resource.trim_start_matches("backend:/"))
}

/// What a read of `path` depends on (RFC 0017): its resource and each path
/// it's under (as [`resource_keys`]), and the record it read
/// (`/api/customers/42` → `backend:/api/customers/42`) or, without a trailing
/// id, the resource's lists and those of the collection it's a part of
/// (`/api/customers/count` is about customers' lists; `/api/customers/12/notes`
/// is under record 12, not a part of customers). `with_lists`: a record's read
/// depends on the lists too.
pub fn read_keys(path: &str, with_lists: bool) -> Vec<String> {
    let mut keys = resource_keys(path);
    let segments = segments(path);
    if segments.last().is_some_and(|s| is_id(s)) {
        keys.push(format!("backend:/{}", segments.join("/")));
        if with_lists {
            keys.push(list_key(path));
        }
    } else {
        keys.push(list_key(path));
        // The collection it's a part of, unless that's a record.
        if let [.., parent, _] = segments.as_slice() {
            if !is_id(parent) {
                let at = segments.len() - 1;
                keys.push(list_key(&format!("/{}", segments[..at].join("/"))));
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    keys.retain(|k| seen.insert(k.clone()));
    keys
}

/// What a write to `path` invalidates (RFC 0017): each record it names and
/// the lists that record is in — `/api/customers/40/archive` is record 40 and
/// customers' lists — and the lists of its own resource. Never the resource
/// key itself, which every view of it reads.
pub fn write_keys(path: &str) -> Vec<String> {
    let segments = segments(path);
    let mut keys: Vec<String> = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        if is_id(segment) && i > 0 {
            keys.push(format!("backend:/{}", segments[..=i].join("/")));
            keys.push(list_key(&format!("/{}", segments[..i].join("/"))));
        }
    }
    keys.push(list_key(path));
    let mut seen = std::collections::BTreeSet::new();
    keys.retain(|k| seen.insert(k.clone()));
    keys
}

/// `path`'s segments, without its query.
fn segments(path: &str) -> Vec<&str> {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// An id in a path: a number, a UUID, or a ULID.
pub(crate) fn is_id(segment: &str) -> bool {
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
    fn a_template_is_filled_from_the_user() {
        let me = serde_json::json!({
            "user": { "id": 7, "uuid": "01J" },
            "workspace": { "id": 3, "slug": "" },
            "teams": [{ "id": 9 }],
        });
        assert_eq!(fill("user.{user.id}", &me).unwrap(), "user.7");
        assert_eq!(
            fill("workspace.{workspace.id}.{ user.uuid }", &me).unwrap(),
            "workspace.3.01J"
        );
        assert_eq!(fill("team.{teams.0.id}", &me).unwrap(), "team.9");
        assert_eq!(fill("announcements", &me).unwrap(), "announcements");
        // What doesn't resolve is named: missing, empty, not a scalar.
        assert_eq!(fill("team.{team.id}", &me).unwrap_err(), "team.id");
        assert_eq!(
            fill("w.{workspace.slug}", &me).unwrap_err(),
            "workspace.slug"
        );
        assert_eq!(fill("w.{workspace}", &me).unwrap_err(), "workspace");
        assert!(fill("w.{workspace.id", &me).is_err());
    }

    #[test]
    fn the_users_id_is_where_the_backend_says() {
        let me = serde_json::json!({ "user": { "id": 7 }, "email": "ada@example.com" });
        let default = Backend::new("https://crm.test/");
        assert_eq!(default.base_url().as_deref(), Some("https://crm.test"));
        assert_eq!(default.user_id(&me), None);
        assert_eq!(
            default.identity(&me).as_deref(),
            Some("ada@example.com"),
            "else the email, as before"
        );
        let nested = Backend::new("https://crm.test").user_id_path("user.id");
        assert_eq!(nested.user_id(&me).as_deref(), Some("7"));
        assert_eq!(nested.identity(&me).as_deref(), Some("7"));
    }

    #[test]
    fn a_server_chosen_at_sign_in_is_shared_by_every_clone() {
        let backend = Backend::chosen_at_sign_in();
        let clone = backend.clone();
        assert_eq!(clone.base_url(), None);
        backend.choose_base(Some("https://acme.test/"));
        assert_eq!(clone.base_url().as_deref(), Some("https://acme.test"));
        backend.choose_base(None);
        assert_eq!(clone.base_url(), None);
        // A fixed one isn't chosen.
        let fixed = Backend::new("https://crm.test");
        fixed.choose_base(Some("https://elsewhere.test"));
        assert_eq!(fixed.base_url().as_deref(), Some("https://crm.test"));
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

    #[test]
    fn a_record_is_read_by_its_own_key_and_a_list_by_the_lists() {
        assert_eq!(
            list_key("/api/customers?page=2"),
            "backend:list:/api/customers"
        );
        assert_eq!(list_key("/api/customers/12"), "backend:list:/api/customers");
        // A record: its resource (and above), and itself, not the lists.
        assert_eq!(
            read_keys("/api/customers/42?include=orders", false),
            [
                "backend:/api/customers",
                "backend:/api",
                "backend:/api/customers/42"
            ]
        );
        // …and the lists too, when asked.
        assert_eq!(
            read_keys("/api/customers/42", true),
            [
                "backend:/api/customers",
                "backend:/api",
                "backend:/api/customers/42",
                "backend:list:/api/customers"
            ]
        );
        // A list: its resource (and above), and its lists.
        assert_eq!(
            read_keys("/api/customers?page=2&search=ada", false),
            [
                "backend:/api/customers",
                "backend:/api",
                "backend:list:/api/customers",
                "backend:list:/api"
            ]
        );
        // A part of the collection: the collection's lists too.
        assert!(read_keys("/api/customers/count", false)
            .contains(&"backend:list:/api/customers".into()));
        assert!(read_keys("/customers/export", false).contains(&"backend:list:/customers".into()));
        // Under a record: that record, not its resource's lists.
        assert_eq!(
            read_keys("/api/customers/12/notes", false),
            [
                "backend:/api/customers/12/notes",
                "backend:/api/customers/12",
                "backend:/api/customers",
                "backend:/api",
                "backend:list:/api/customers/12/notes"
            ]
        );
        assert_eq!(
            read_keys("/api/orders/9b2f5c1e-3d4a-4e5f-8a9b-0c1d2e3f4a5b", false)[2],
            "backend:/api/orders/9b2f5c1e-3d4a-4e5f-8a9b-0c1d2e3f4a5b"
        );
    }

    #[test]
    fn a_write_invalidates_the_records_it_names_and_their_lists() {
        assert_eq!(
            write_keys("/api/customers/40"),
            ["backend:/api/customers/40", "backend:list:/api/customers"]
        );
        // A new one: no view has read it yet.
        assert_eq!(
            write_keys("/api/customers"),
            ["backend:list:/api/customers"]
        );
        // A record's action is a change to it.
        assert_eq!(
            write_keys("/api/customers/40/archive?force=1"),
            [
                "backend:/api/customers/40",
                "backend:list:/api/customers",
                "backend:list:/api/customers/40/archive"
            ]
        );
        // Under a record: its own record, and the one it's under.
        assert_eq!(
            write_keys("/api/customers/12/notes/5"),
            [
                "backend:/api/customers/12",
                "backend:list:/api/customers",
                "backend:/api/customers/12/notes/5",
                "backend:list:/api/customers/12/notes"
            ]
        );
        // Another resource's write isn't this one's.
        assert_eq!(write_keys("/api/orders"), ["backend:list:/api/orders"]);
    }
}
