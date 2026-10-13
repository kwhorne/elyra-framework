//! Laravel's notifications on the desktop (RFC 0008): what
//! `$user->notify(..)` sends through the `broadcast` channel, heard on the
//! user's private channel (`App.Models.User.{id}`) once they're signed in.
//! Behind the `backend` feature.
//!
//! ```ignore
//! Backend::new("https://crm.example.com")
//!     .broadcasting(Broadcasting::pusher(key))
//!     .notifications()
//! ```
//!
//! Each one heard is a [`NotificationReceived`] — a domain event — and goes
//! to the windows on `elyra:notification`. Laravel doesn't say what a
//! notification's title is: by convention it's `title`, `body` and `url` in
//! what `toArray` returned, and [`Backend::present_notifications`] reads
//! anything else.
//!
//! [`Backend::present_notifications`]: crate::backend::Backend::present_notifications

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::Value;

use crate::backend::BackendError;
use crate::notifier::{Notifier, Shown};

use crate::container::{Ctx, WeakCtx};
use crate::event::EventBus;

/// The channel each notification reaches the windows on.
pub const CHANNEL: &str = "elyra:notification";

/// The channel a click on a native notification reaches the windows on.
pub const CLICKED_CHANNEL: &str = "elyra:notification-clicked";

/// The channel an in-app path reaches the router on, to be opened.
pub const NAVIGATE_CHANNEL: &str = "elyra:navigate";

/// The channel the list and the unread count reach the windows on.
pub const STATE_CHANNEL: &str = "elyra:notifications";

/// What the web's "mark as read" broadcasts on the user's channel, from the
/// controller rata prints: the list is read again.
pub const READ_EVENT: &str = "NotificationsRead";

/// The event Laravel broadcasts a notification as — what Echo's
/// `.notification(..)` listens for.
pub const BROADCAST_EVENT: &str = "Illuminate\\Notifications\\Events\\BroadcastNotificationCreated";

/// How a notification is shown: from its data.
pub type Presenter = Arc<dyn Fn(&Notification) -> Presented + Send + Sync>;

/// `.notifications()` on a `Backend`: where the list is, which channel is the
/// user's, and how a notification is shown.
#[derive(Clone)]
pub struct Config {
    /// The `database` channel's list (`GET`), and its reads.
    pub route: String,
    /// The user's channel, `{id}` filled with the signed-in user's id.
    pub channel: String,
    /// Whether a native notification shows its title and body, or only "New
    /// notification" — what a locked screen may show.
    pub previews: bool,
    /// Whether the unread count is on the app's icon and in the tray.
    pub badge: bool,
    /// The app's page listing them: where "4 new notifications" opens.
    pub page: String,
    pub(crate) presenter: Presenter,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            route: "/api/notifications".into(),
            channel: "App.Models.User.{id}".into(),
            previews: true,
            badge: true,
            page: "/notifications".into(),
            presenter: Arc::new(Presented::from_data),
        }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("route", &self.route)
            .field("channel", &self.channel)
            .field("previews", &self.previews)
            .field("badge", &self.badge)
            .finish_non_exhaustive()
    }
}

impl Config {
    /// The user's channel for `user` (as `/api/user` answered): `None` when
    /// the channel needs an id and the user has none.
    pub(crate) fn channel_for(&self, user: &Value) -> Option<String> {
        if !self.channel.contains("{id}") {
            return Some(self.channel.clone());
        }
        let id = match &user["id"] {
            Value::Number(n) => n.to_string(),
            Value::String(s) if !s.is_empty() => s.clone(),
            _ => return None,
        };
        Some(self.channel.replace("{id}", &id))
    }
}

/// A notification, as Laravel keeps and broadcasts it. Its data is the
/// server's: untrusted, like any input.
/// It reads Laravel's rows (`type`, `read_at`, `created_at`) as well as its
/// own camelCase.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// Laravel's UUID for it.
    pub id: String,
    /// Its `type`: the notification's class, or its `broadcastType()`.
    #[serde(alias = "type", default)]
    pub kind: String,
    /// What `toArray` (or `toBroadcast`) returned.
    #[serde(default)]
    pub data: Value,
    #[serde(alias = "read_at", default)]
    pub read_at: Option<String>,
    #[serde(alias = "created_at", default)]
    pub created_at: Option<String>,
}

impl Notification {
    /// One from a broadcast's payload: the data, with `id` and `type` beside
    /// it. `None` without an id.
    pub(crate) fn from_broadcast(payload: Value) -> Option<Self> {
        let Value::Object(mut data) = payload else {
            return None;
        };
        let id = match data.remove("id")? {
            Value::String(id) if !id.is_empty() => id,
            _ => return None,
        };
        let kind = match data.remove("type") {
            Some(Value::String(kind)) => kind,
            _ => String::new(),
        };
        Some(Self {
            id,
            kind,
            data: Value::Object(data),
            read_at: None,
            created_at: None,
        })
    }
}

/// What the OS and the app show for a notification.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Presented {
    pub title: String,
    pub body: Option<String>,
    /// Where a click goes: an in-app path (`/invoices/7`) is opened; anything
    /// else is left to the app.
    pub url: Option<String>,
}

impl Presented {
    /// The convention: `title`, `body` and `url` in the data.
    pub fn from_data(notification: &Notification) -> Self {
        let text = |key: &str| {
            notification.data[key]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        Self {
            title: text("title").unwrap_or_else(|| "New notification".into()),
            body: text("body"),
            url: text("url"),
        }
    }
}

/// A notification arrived: dispatched as a domain event
/// (`App::listen(|e: NotificationReceived, ctx| …)`) and sent to the windows
/// on `elyra:notification`.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct NotificationReceived {
    pub notification: Notification,
    pub presented: Presented,
    /// Where it was shown.
    pub shown: ShownAs,
}

/// Where a notification is shown: in the OS when no window of the app has
/// focus, as a toast in the app when one has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ShownAs {
    Native,
    Toast,
}

/// A native notification was clicked: dispatched as a domain event, and sent
/// to the windows on `elyra:notification-clicked`. An in-app `url` is opened
/// already; anything else is the app's to open, or not.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct NotificationClicked {
    pub notification: Notification,
    pub presented: Presented,
}

/// `url` when it's a path in the app (`/invoices/7`): never a full URL, a
/// protocol-relative one (`//host`), or anything else a click shouldn't open
/// by itself.
pub fn in_app_path(url: &str) -> Option<&str> {
    let ok = url.starts_with('/')
        && !url.starts_with("//")
        && url.len() <= 2048
        && !url.chars().any(|c| c == '\\' || c.is_control());
    ok.then_some(url)
}

/// The list as the app has it, and the unread count — what
/// `elyra:notifications` carries and the runtime's `notifications` store
/// holds.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsState {
    pub unread: u32,
    /// Newest first: the pages read so far, and what was heard since.
    pub items: Vec<Notification>,
    pub has_more: bool,
}

/// The user's notifications. Bound by `App` when the backend has
/// `.notifications()`.
#[derive(Clone)]
pub struct Notifications {
    inner: Arc<Inner>,
}

struct Inner {
    config: Config,
    bus: Option<EventBus>,
    ctx: std::sync::OnceLock<WeakCtx>,
    unread: AtomicU32,
    list: parking_lot::Mutex<List>,
    /// One read of the list at a time.
    reading: tokio::sync::Mutex<()>,
    /// Where background work runs: a click arrives on a thread of the OS's.
    runtime: std::sync::OnceLock<tokio::runtime::Handle>,
}

#[derive(Default)]
struct List {
    items: Vec<Notification>,
    has_more: bool,
    /// The last page read.
    page: u32,
    /// Heard (and shown) in this run: never "missed".
    heard: std::collections::HashSet<String>,
    /// Read here, and not yet read on the server as far as a page said: a
    /// list read meanwhile (or its offline copy) doesn't undo it.
    read_here: std::collections::HashSet<String>,
    /// All read here, at this time, until the server's count agrees.
    all_read_here: Option<String>,
}

impl List {
    /// The first page from the server, with what was read here laid over it;
    /// the unread count to show, from the server's.
    fn read_over(&mut self, items: &mut [Notification], server_unread: u32) -> u32 {
        let masked = self.mask(items);
        if self.all_read_here.is_some() {
            let newer = items.iter().filter(|n| n.read_at.is_none()).count() as u32;
            if server_unread <= newer {
                self.all_read_here = None; // the server caught up
            }
            return newer;
        }
        server_unread.saturating_sub(masked)
    }

    /// Mark read what was read here; how many that was. One the server shows
    /// read is no longer this app's to remember.
    fn mask(&mut self, items: &mut [Notification]) -> u32 {
        let mut masked = 0u32;
        for n in items.iter_mut() {
            if n.read_at.is_some() {
                self.read_here.remove(&n.id);
                continue;
            }
            let before_all = self.all_read_here.as_deref().is_some_and(|at| {
                n.created_at
                    .as_deref()
                    .is_none_or(|created| !later(created, at))
            });
            if self.read_here.contains(&n.id) || before_all {
                n.read_at = Some(jiff::Timestamp::now().to_string());
                masked += 1;
            }
        }
        masked
    }
}

/// One page of `GET /api/notifications`: Laravel's paginator, with `unread`
/// beside it (the controller rata prints).
struct Page {
    items: Vec<Notification>,
    has_more: bool,
    unread: Option<u32>,
}

impl Page {
    fn from(value: &Value) -> Self {
        let items = value["data"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| serde_json::from_value(row.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        let has_more = match (value["current_page"].as_u64(), value["last_page"].as_u64()) {
            (Some(current), Some(last)) => current < last,
            _ => value["next_page_url"].is_string(),
        };
        Self {
            items,
            has_more,
            unread: value["unread"]
                .as_u64()
                .map(|n| n.min(u32::MAX as u64) as u32),
        }
    }
}

/// Whether `a` is later than `b`, both Laravel timestamps.
fn later(a: &str, b: &str) -> bool {
    match (a.parse::<jiff::Timestamp>(), b.parse::<jiff::Timestamp>()) {
        (Ok(a), Ok(b)) => a > b,
        _ => a > b,
    }
}

/// The settings key for the newest notification the app knew of, per backend.
fn seen_key(route: &str) -> String {
    format!("elyra.notifications.seen:{route}")
}

impl Notifications {
    pub(crate) fn new(config: Config, bus: Option<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                bus,
                ctx: std::sync::OnceLock::new(),
                unread: AtomicU32::new(0),
                list: parking_lot::Mutex::new(List::default()),
                reading: tokio::sync::Mutex::new(()),
                runtime: std::sync::OnceLock::new(),
            }),
        }
    }

    pub(crate) fn attach(&self, ctx: &Ctx) {
        let _ = self.inner.ctx.set(ctx.downgrade());
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let _ = self.inner.runtime.set(handle);
        }
    }

    /// Run `work` in the background, on the app's runtime.
    pub(crate) fn spawn<F>(&self, work: impl FnOnce(Self) -> F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = self
            .inner
            .runtime
            .get()
            .cloned()
            .or_else(|| tokio::runtime::Handle::try_current().ok());
        if let Some(handle) = handle {
            handle.spawn(work(self.clone()));
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// How `notification` is shown.
    pub fn present(&self, notification: &Notification) -> Presented {
        (self.inner.config.presenter)(notification)
    }

    /// A broadcast on the user's channel said one was created.
    pub(crate) fn heard(&self, payload: Value) {
        let Some(notification) = Notification::from_broadcast(payload) else {
            crate::warn!(target: "elyra::notifications", "a notification without an id: ignored");
            return;
        };
        let focused = self
            .ctx()
            .and_then(|ctx| ctx.try_get::<crate::window::Focus>())
            .is_some_and(|f| f.is_focused());
        let received = NotificationReceived {
            presented: self.present(&notification),
            notification,
            shown: if focused {
                ShownAs::Toast
            } else {
                ShownAs::Native
            },
        };
        let new = {
            let mut list = self.inner.list.lock();
            list.heard.insert(received.notification.id.clone());
            let known = list.items.iter().any(|n| n.id == received.notification.id);
            if !known {
                list.items.insert(0, received.notification.clone());
            }
            !known
        };
        if new {
            self.set_unread(self.unread() + 1);
        }
        self.emit_state();
        // The server's list has it now, with its date and the exact count.
        self.spawn(|me| async move {
            let _ = me.refresh().await;
        });
        crate::debug!(
            target: "elyra::notifications",
            "{} ({})",
            received.notification.kind,
            received.notification.id
        );
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit(CHANNEL, &received);
        }
        if received.shown == ShownAs::Native {
            self.show_native(&received);
        }
        if let Some(ctx) = self.ctx() {
            ctx.dispatch_background(received);
        }
    }

    /// How many are unread, as far as the app knows.
    pub fn unread(&self) -> u32 {
        self.inner.unread.load(Ordering::SeqCst)
    }

    /// The unread count, on the app's icon and in the tray unless
    /// `.notification_badge(false)`.
    pub(crate) fn set_unread(&self, count: u32) {
        self.inner.unread.store(count, Ordering::SeqCst);
        if self.inner.config.badge {
            if let Some(windows) = self
                .ctx()
                .and_then(|c| c.try_get::<crate::window::Windows>())
            {
                windows.set_badge(count);
            }
        }
    }

    /// The user signed out: nothing of theirs is kept or counted.
    pub(crate) fn signed_out(&self) {
        *self.inner.list.lock() = List::default();
        self.set_unread(0);
        self.emit_state();
    }

    /// The list as the app has it.
    pub fn state(&self) -> NotificationsState {
        let list = self.inner.list.lock();
        NotificationsState {
            unread: self.unread(),
            items: list.items.clone(),
            has_more: list.has_more,
        }
    }

    fn emit_state(&self) {
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit(STATE_CHANNEL, &self.state());
        }
    }

    fn backend(&self) -> Result<Arc<crate::backend::Backend>, BackendError> {
        self.ctx()
            .and_then(|c| c.try_get::<crate::backend::Backend>())
            .ok_or_else(|| BackendError::Other {
                status: 0,
                message: "the app has no backend".into(),
            })
    }

    async fn page(&self, page: u32) -> Result<Page, BackendError> {
        let value: Value = self
            .backend()?
            .get(&self.inner.config.route)
            .query(&serde_json::json!({ "page": page }))
            .json()
            .await?;
        Ok(Page::from(&value))
    }

    /// Read the first page again (offline: the last copy, RFC 0007), with
    /// the unread count.
    pub async fn refresh(&self) -> Result<NotificationsState, BackendError> {
        let _one = self.inner.reading.lock().await;
        let page = self.page(1).await?;
        let newest = page
            .items
            .iter()
            .filter_map(|n| n.created_at.clone())
            .reduce(|a, b| if later(&b, &a) { b } else { a });
        let server_unread = page
            .unread
            .unwrap_or_else(|| page.items.iter().filter(|n| n.read_at.is_none()).count() as u32);
        let mut items = page.items;
        let unread = {
            let mut list = self.inner.list.lock();
            let unread = list.read_over(&mut items, server_unread);
            list.items = items;
            list.has_more = page.has_more;
            list.page = 1;
            unread
        };
        if let Some(newest) = newest {
            self.remember_seen(&newest).await;
        }
        self.set_unread(unread);
        self.emit_state();
        Ok(self.state())
    }

    /// The next page, after those read so far.
    pub async fn more(&self) -> Result<NotificationsState, BackendError> {
        let _one = self.inner.reading.lock().await;
        let next = self.inner.list.lock().page.max(1) + 1;
        let mut page = self.page(next).await?;
        {
            let mut list = self.inner.list.lock();
            list.mask(&mut page.items);
            for n in page.items {
                if !list.items.iter().any(|known| known.id == n.id) {
                    list.items.push(n);
                }
            }
            list.has_more = page.has_more;
            list.page = next;
        }
        self.emit_state();
        Ok(self.state())
    }

    /// Mark one read, here at once and on the server — through the outbox
    /// when it can't be reached (marking twice is harmless).
    pub async fn mark_read(&self, id: &str) -> Result<(), BackendError> {
        let was_unread = {
            let mut list = self.inner.list.lock();
            list.read_here.insert(id.to_owned());
            match list.items.iter_mut().find(|n| n.id == id) {
                Some(n) if n.read_at.is_none() => {
                    n.read_at = Some(jiff::Timestamp::now().to_string());
                    true
                }
                Some(_) => false,
                // Not on a page read so far: the server's count says.
                None => true,
            }
        };
        if was_unread {
            self.set_unread(self.unread().saturating_sub(1));
        }
        self.emit_state();
        let route = format!("{}/{}/read", self.inner.config.route, id);
        self.send(route).await
    }

    /// Mark every one read.
    pub async fn mark_all_read(&self) -> Result<(), BackendError> {
        {
            let now = jiff::Timestamp::now().to_string();
            let mut list = self.inner.list.lock();
            for n in list.items.iter_mut().filter(|n| n.read_at.is_none()) {
                n.read_at = Some(now.clone());
            }
            list.all_read_here = Some(now);
        }
        self.set_unread(0);
        self.emit_state();
        let route = format!("{}/read-all", self.inner.config.route);
        self.send(route).await
    }

    async fn send(&self, route: String) -> Result<(), BackendError> {
        match self.backend()?.post(route).queue_offline().send().await {
            Ok(_) | Err(BackendError::Queued(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Connected again, or at the start: read the list, and show what came
    /// while the app wasn't listening — one as itself, more as "4 new
    /// notifications" — unless this user has never been seen here.
    pub(crate) async fn catch_up(&self) {
        let before = self.seen().await;
        if let Err(e) = self.refresh().await {
            crate::debug!(target: "elyra::notifications", "couldn't read the list: {e}");
            return;
        }
        let Some(before) = before else {
            return;
        };
        let focused = self
            .ctx()
            .and_then(|ctx| ctx.try_get::<crate::window::Focus>())
            .is_some_and(|f| f.is_focused());
        if focused {
            return; // the list is in front of them
        }
        let missed: Vec<Notification> = {
            let list = self.inner.list.lock();
            list.items
                .iter()
                .filter(|n| n.read_at.is_none() && !list.heard.contains(&n.id))
                .filter(|n| n.created_at.as_deref().is_some_and(|at| later(at, &before)))
                .cloned()
                .collect()
        };
        match missed.as_slice() {
            [] => {}
            [one] => {
                let received = NotificationReceived {
                    presented: self.present(one),
                    notification: one.clone(),
                    shown: ShownAs::Native,
                };
                self.show_native(&received);
            }
            many => self.show_summary(many.len()),
        }
        let mut list = self.inner.list.lock();
        for n in &missed {
            list.heard.insert(n.id.clone());
        }
    }

    /// The newest notification this user's list had, the last time it was
    /// read here.
    async fn seen(&self) -> Option<String> {
        let (store, user) = self.store_and_user().await?;
        let seen = store.get(&seen_key(&self.inner.config.route))?;
        (seen["user"].as_str() == Some(user.as_str()))
            .then(|| seen["at"].as_str().map(str::to_owned))
            .flatten()
    }

    async fn remember_seen(&self, newest: &str) {
        let Some((store, user)) = self.store_and_user().await else {
            return;
        };
        let key = seen_key(&self.inner.config.route);
        let keep = match store.get(&key) {
            Some(seen) if seen["user"].as_str() == Some(user.as_str()) => seen["at"]
                .as_str()
                .is_none_or(|before| later(newest, before)),
            _ => true,
        };
        if keep {
            store.set(key, serde_json::json!({ "user": user, "at": newest }));
        }
    }

    async fn store_and_user(&self) -> Option<(Arc<crate::store::Store>, String)> {
        let ctx = self.ctx()?;
        let user: Value = ctx.try_get::<crate::auth::Auth>()?.user().await.ok()?;
        let user = match &user["id"] {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => user["email"].as_str()?.to_owned(),
        };
        Some((ctx.try_get::<crate::store::Store>()?, user))
    }

    /// "4 new notifications", which opens the app's list.
    fn show_summary(&self, count: usize) {
        let Some(notifier) = self.ctx().and_then(|c| c.try_get::<Notifier>()) else {
            return;
        };
        let me = self.clone();
        let shown = Shown {
            title: format!("{count} new notifications"),
            body: None,
        };
        let opened = move || {
            let Some(ctx) = me.ctx() else {
                return;
            };
            if let Some(windows) = ctx.try_get::<crate::window::Windows>() {
                windows.show(None);
                windows.focus(None);
            }
            if let (Some(bus), Some(page)) = (&me.inner.bus, in_app_path(&me.inner.config.page)) {
                let _ = bus.emit(NAVIGATE_CHANNEL, &page);
            }
        };
        if let Err(e) = notifier.show(shown, Some(Box::new(opened))) {
            crate::debug!(target: "elyra::notifications", "not shown in the OS: {e}");
        }
    }

    fn ctx(&self) -> Option<Ctx> {
        self.inner.ctx.get().and_then(WeakCtx::upgrade)
    }

    /// In the OS, through the container's `Notifier`; a click comes back to
    /// [`clicked`](Self::clicked).
    fn show_native(&self, received: &NotificationReceived) {
        let Some(notifier) = self.ctx().and_then(|c| c.try_get::<Notifier>()) else {
            return;
        };
        let shown = if self.inner.config.previews {
            Shown {
                title: received.presented.title.clone(),
                body: received.presented.body.clone(),
            }
        } else {
            Shown {
                title: "New notification".into(),
                body: None,
            }
        };
        let (me, clicked) = (self.clone(), received.clone());
        if let Err(e) = notifier.show(shown, Some(Box::new(move || me.clicked(clicked)))) {
            crate::debug!(target: "elyra::notifications", "not shown in the OS: {e}");
        }
    }

    /// The user clicked a native notification: the app comes forward, an
    /// in-app `url` opens, and it's announced.
    fn clicked(&self, received: NotificationReceived) {
        let Some(ctx) = self.ctx() else {
            return;
        };
        if let Some(windows) = ctx.try_get::<crate::window::Windows>() {
            windows.show(None);
            windows.focus(None);
        }
        // Opened is read.
        let id = received.notification.id.clone();
        self.spawn(|me| async move {
            if let Err(e) = me.mark_read(&id).await {
                crate::debug!(target: "elyra::notifications", "couldn't mark it read: {e}");
            }
        });
        let clicked = NotificationClicked {
            notification: received.notification,
            presented: received.presented,
        };
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit(CLICKED_CHANNEL, &clicked);
            if let Some(path) = clicked.presented.url.as_deref().and_then(in_app_path) {
                let _ = bus.emit(NAVIGATE_CHANNEL, &path);
            }
        }
        ctx.dispatch_background(clicked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_broadcast_is_the_data_with_its_id_and_type_beside_it() {
        let n = Notification::from_broadcast(json!({
            "id": "9b1", "type": "invoice-paid", "title": "Paid", "amount": 40
        }))
        .unwrap();
        assert_eq!(n.id, "9b1");
        assert_eq!(n.kind, "invoice-paid");
        assert_eq!(n.data, json!({ "title": "Paid", "amount": 40 }));
        assert!(Notification::from_broadcast(json!({ "title": "no id" })).is_none());
        assert!(Notification::from_broadcast(json!({ "id": "" })).is_none());
        assert!(Notification::from_broadcast(json!("text")).is_none());
    }

    #[test]
    fn title_body_and_url_by_convention() {
        let n = |data| Notification {
            id: "1".into(),
            kind: String::new(),
            data,
            read_at: None,
            created_at: None,
        };
        assert_eq!(
            Presented::from_data(&n(
                json!({ "title": " Paid ", "body": "Invoice 7", "url": "/invoices/7" })
            )),
            Presented {
                title: "Paid".into(),
                body: Some("Invoice 7".into()),
                url: Some("/invoices/7".into()),
            }
        );
        let bare = Presented::from_data(&n(json!({ "title": 4, "body": "" })));
        assert_eq!(bare.title, "New notification");
        assert_eq!(bare.body, None);
    }

    #[test]
    fn only_a_path_in_the_app_is_opened_by_a_click() {
        assert_eq!(in_app_path("/invoices/7"), Some("/invoices/7"));
        assert_eq!(in_app_path("/customers?page=2"), Some("/customers?page=2"));
        for no in [
            "https://evil.example/",
            "//evil.example/x",
            "javascript:alert(1)",
            "invoices/7",
            "/\\evil.example",
            "/a\nb",
            "",
        ] {
            assert_eq!(in_app_path(no), None, "{no:?}");
        }
        assert_eq!(in_app_path(&format!("/{}", "a".repeat(3000))), None);
    }

    #[test]
    fn the_users_channel_has_their_id() {
        let config = Config::default();
        assert_eq!(
            config.channel_for(&json!({ "id": 7 })).as_deref(),
            Some("App.Models.User.7")
        );
        assert_eq!(
            config.channel_for(&json!({ "id": "01J" })).as_deref(),
            Some("App.Models.User.01J")
        );
        assert_eq!(config.channel_for(&json!({ "email": "a@b" })), None);
        let fixed = Config {
            channel: "inbox".into(),
            ..Config::default()
        };
        assert_eq!(fixed.channel_for(&json!({})).as_deref(), Some("inbox"));
    }
}
