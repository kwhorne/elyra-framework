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

use std::sync::Arc;

use serde_json::Value;

use crate::container::{Ctx, WeakCtx};
use crate::event::EventBus;

/// The channel each notification reaches the windows on.
pub const CHANNEL: &str = "elyra:notification";

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
    pub(crate) presenter: Presenter,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            route: "/api/notifications".into(),
            channel: "App.Models.User.{id}".into(),
            presenter: Arc::new(Presented::from_data),
        }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("route", &self.route)
            .field("channel", &self.channel)
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
}

impl Notifications {
    pub(crate) fn new(config: Config, bus: Option<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                bus,
                ctx: std::sync::OnceLock::new(),
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
        let received = NotificationReceived {
            presented: self.present(&notification),
            notification,
        };
        crate::debug!(
            target: "elyra::notifications",
            "{} ({})",
            received.notification.kind,
            received.notification.id
        );
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit(CHANNEL, &received);
        }
        if let Some(ctx) = self.inner.ctx.get().and_then(WeakCtx::upgrade) {
            ctx.dispatch_background(received);
        }
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
