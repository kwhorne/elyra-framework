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

use crate::notifier::{Notifier, Shown};

use crate::container::{Ctx, WeakCtx};
use crate::event::EventBus;

/// The channel each notification reaches the windows on.
pub const CHANNEL: &str = "elyra:notification";

/// The channel a click on a native notification reaches the windows on.
pub const CLICKED_CHANNEL: &str = "elyra:notification-clicked";

/// The channel an in-app path reaches the router on, to be opened.
pub const NAVIGATE_CHANNEL: &str = "elyra:navigate";

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
    pub(crate) presenter: Presenter,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            route: "/api/notifications".into(),
            channel: "App.Models.User.{id}".into(),
            previews: true,
            badge: true,
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
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// Laravel's UUID for it.
    pub id: String,
    /// Its `type`: the notification's class, or its `broadcastType()`.
    pub kind: String,
    /// What `toArray` (or `toBroadcast`) returned.
    pub data: Value,
    pub read_at: Option<String>,
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
}

impl Notifications {
    pub(crate) fn new(config: Config, bus: Option<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                bus,
                ctx: std::sync::OnceLock::new(),
                unread: AtomicU32::new(0),
            }),
        }
    }

    pub(crate) fn attach(&self, ctx: &Ctx) {
        let _ = self.inner.ctx.set(ctx.downgrade());
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
        self.set_unread(self.unread() + 1);
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

    /// The user signed out: nothing of theirs is counted.
    pub(crate) fn signed_out(&self) {
        self.set_unread(0);
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
