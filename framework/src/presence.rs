//! Presence (RFC 0015): who else has a record open, on the web or the
//! desktop. The app joins Laravel's presence channels the way Echo does in
//! a browser (`presence-customers.42`), so a browser tab and a desktop
//! window on the same record see each other.
//!
//! ```ignore
//! Backend::new("https://crm.example.com")
//!     .broadcasting(Broadcasting::pusher("app-key"))
//!     .presence(["customers.*"])     // the channels a window may join
//! ```
//!
//! - A window joins with `/__presence/join` `{ channel }` (the runtime's
//!   `presence(channel)` store does it) and leaves with `/__presence/leave`.
//!   A channel is joined once however many windows want it, and left when
//!   the last one leaves, or stops polling (its window closed).
//! - Laravel decides who may join, in `routes/channels.php`, and what each
//!   member's `info` is. The app decides which channels it asks about:
//!   only names matching `Backend::presence(..)`.
//! - The members go to the windows that joined, on `elyra:presence`:
//!   `{ channel, joined, members: [{ id, info }], me }`. Without a
//!   connection (signed out, offline, unplugged) a channel is `joined:
//!   false`, with no members, and it's joined again when the connection is
//!   back.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::event::EventBus;

/// The channel the members reach the windows on.
pub const CHANNEL: &str = "elyra:presence";

/// How often windows that stopped polling are let go of.
const SWEEP_EVERY: Duration = Duration::from_secs(30);

/// A window that joined this recently isn't let go of for not having polled
/// yet.
const GRACE: Duration = Duration::from_secs(60);

/// Someone in a channel: Laravel's `user_id`, and the `user_info` its
/// channel rule returned (a name, an avatar).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Member {
    pub id: String,
    pub info: Value,
}

/// A channel, as a window sees it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresenceState {
    /// The name the app uses (`customers.42`), not the wire's.
    pub channel: String,
    /// Whether the server has said who's there.
    pub joined: bool,
    /// Everyone there, the signed-in user included, by id.
    pub members: Vec<Member>,
    /// The signed-in user's id, to tell them apart.
    pub me: Option<String>,
}

/// What a window asks the open connection to do.
#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    Join(String),
    Leave(String),
}

/// The channels the windows want, and who's in each. Shared by the shell's
/// routes and the broadcasting connection.
#[derive(Clone)]
pub struct Presence {
    inner: Arc<Inner>,
}

struct Inner {
    patterns: Vec<String>,
    /// Each wanted channel's windows, with when each joined.
    windows: Mutex<HashMap<String, HashMap<String, Instant>>>,
    /// The members of each channel the server has answered for.
    members: Mutex<HashMap<String, BTreeMap<String, Value>>>,
    /// The open connection's queue; `None` while there's none.
    session: Mutex<Option<UnboundedSender<Command>>>,
    me: RwLock<Option<String>>,
    bus: Option<EventBus>,
    sweeping: AtomicBool,
}

/// A presence channel's wire name, `presence-` and the app's name.
pub(crate) fn wire(channel: &str) -> String {
    format!("presence-{channel}")
}

/// The app's name for a channel on the wire, if it's a presence channel.
pub(crate) fn from_wire(wire: &str) -> Option<&str> {
    wire.strip_prefix("presence-")
}

/// Pusher's channel names: letters, digits and `-_=@,.;`, at most 164
/// characters with the prefix.
fn valid(channel: &str) -> bool {
    !channel.is_empty()
        && wire(channel).len() <= 164
        && channel
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_=@,.;".contains(c))
}

impl Presence {
    pub(crate) fn new(patterns: Vec<String>, bus: Option<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                patterns,
                windows: Mutex::new(HashMap::new()),
                members: Mutex::new(HashMap::new()),
                session: Mutex::new(None),
                me: RwLock::new(None),
                bus,
                sweeping: AtomicBool::new(false),
            }),
        }
    }

    /// Whether a window may join `channel`: a valid name that one of
    /// `Backend::presence(..)`'s patterns covers.
    pub fn allows(&self, channel: &str) -> bool {
        valid(channel)
            && self
                .inner
                .patterns
                .iter()
                .any(|p| crate::security::ability_matches(p, channel))
    }

    /// `window` wants `channel`: joined on the open connection if no other
    /// window had it yet. What's known of it now.
    pub fn join(&self, window: &str, channel: &str) -> Result<PresenceState, String> {
        if !self.allows(channel) {
            return Err(format!(
                "`{channel}` isn't a presence channel this app joins (Backend::presence)"
            ));
        }
        let first = {
            let mut windows = self.inner.windows.lock();
            let wanting = windows.entry(channel.to_owned()).or_default();
            let first = wanting.is_empty();
            wanting
                .entry(window.to_owned())
                .or_insert_with(Instant::now);
            first
        };
        if first {
            self.send(Command::Join(channel.to_owned()));
        }
        self.keep_sweeping();
        Ok(self.state(channel))
    }

    /// `window` is done with `channel`: left on the connection when no
    /// window wants it any more. Whether `window` had it.
    pub fn leave(&self, window: &str, channel: &str) -> bool {
        let (had, last) = {
            let mut windows = self.inner.windows.lock();
            let Some(wanting) = windows.get_mut(channel) else {
                return false;
            };
            let had = wanting.remove(window).is_some();
            let last = wanting.is_empty();
            if last {
                windows.remove(channel);
            }
            (had, last)
        };
        if last {
            self.inner.members.lock().remove(channel);
            self.send(Command::Leave(channel.to_owned()));
        }
        had
    }

    /// Who's in `channel` now, by id; empty when it isn't joined.
    pub fn members(&self, channel: &str) -> Vec<Member> {
        self.state(channel).members
    }

    /// The channels some window wants, in order.
    pub fn channels(&self) -> Vec<String> {
        let mut channels: Vec<String> = self.inner.windows.lock().keys().cloned().collect();
        channels.sort();
        channels
    }

    /// `channel` as a window sees it.
    pub fn state(&self, channel: &str) -> PresenceState {
        let members = self.inner.members.lock();
        let known = members.get(channel);
        PresenceState {
            channel: channel.to_owned(),
            joined: known.is_some(),
            members: known
                .map(|m| {
                    m.iter()
                        .map(|(id, info)| Member {
                            id: id.clone(),
                            info: info.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            me: self.inner.me.read().clone(),
        }
    }

    /// Let go of the windows that stopped polling — their windows closed —
    /// unless they joined within `grace`; channels no window wants any more
    /// are left. Runs every 30 s while anything is joined.
    #[doc(hidden)]
    pub fn sweep(&self, grace: Duration) {
        let Some(bus) = &self.inner.bus else {
            return;
        };
        let gone: Vec<(String, String)> = self
            .inner
            .windows
            .lock()
            .iter()
            .flat_map(|(channel, wanting)| {
                wanting
                    .iter()
                    .filter(|(window, since)| since.elapsed() >= grace && !bus.is_connected(window))
                    .map(|(window, _)| (channel.clone(), window.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (channel, window) in gone {
            self.leave(&window, &channel);
        }
    }

    fn keep_sweeping(&self) {
        if self.inner.bus.is_none() || self.inner.sweeping.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.inner.sweeping.store(false, Ordering::SeqCst);
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        handle.spawn(async move {
            loop {
                tokio::time::sleep(SWEEP_EVERY).await;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let presence = Presence { inner };
                presence.sweep(GRACE);
                if presence.inner.windows.lock().is_empty() {
                    presence.inner.sweeping.store(false, Ordering::SeqCst);
                    return;
                }
            }
        });
    }

    fn send(&self, command: Command) {
        if let Some(session) = &*self.inner.session.lock() {
            let _ = session.send(command);
        }
    }

    /// Tell the windows that want `channel` how it is now.
    fn emit(&self, channel: &str) {
        let Some(bus) = &self.inner.bus else {
            return;
        };
        let state = self.state(channel);
        let windows: Vec<String> = self
            .inner
            .windows
            .lock()
            .get(channel)
            .map(|w| w.keys().cloned().collect())
            .unwrap_or_default();
        for window in windows {
            let _ = bus.emit_to(&window, CHANNEL, &state);
        }
    }

    // -- the connection's side ------------------------------------------------

    /// A connection opened: it takes the windows' joins and leaves from now
    /// on. The channels to join on it now.
    pub(crate) fn attach(
        &self,
        session: UnboundedSender<Command>,
        me: Option<String>,
    ) -> Vec<String> {
        *self.inner.me.write() = me;
        *self.inner.session.lock() = Some(session);
        self.channels()
    }

    /// The connection closed: nobody is known to be anywhere until the next
    /// one says so.
    pub(crate) fn detach(&self) {
        *self.inner.session.lock() = None;
        let known: Vec<String> = self.inner.members.lock().drain().map(|(c, _)| c).collect();
        for channel in known {
            self.emit(&channel);
        }
    }

    /// The server said who's there, as `pusher_internal:subscription_succeeded`
    /// carries it: `{ presence: { ids, hash, count } }`.
    pub(crate) fn subscribed(&self, channel: &str, data: &Value) {
        if !self.inner.windows.lock().contains_key(channel) {
            return;
        }
        let members: BTreeMap<String, Value> = data["presence"]["hash"]
            .as_object()
            .map(|hash| {
                hash.iter()
                    .map(|(id, info)| (id.clone(), info.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.inner
            .members
            .lock()
            .insert(channel.to_owned(), members);
        self.emit(channel);
    }

    /// `pusher_internal:member_added`: `{ user_id, user_info }`.
    pub(crate) fn added(&self, channel: &str, data: &Value) {
        let Some(id) = user_id(&data["user_id"]) else {
            return;
        };
        {
            let mut members = self.inner.members.lock();
            let Some(known) = members.get_mut(channel) else {
                return;
            };
            known.insert(id, data["user_info"].clone());
        }
        self.emit(channel);
    }

    /// `pusher_internal:member_removed`: `{ user_id }`.
    pub(crate) fn removed(&self, channel: &str, data: &Value) {
        let Some(id) = user_id(&data["user_id"]) else {
            return;
        };
        let changed = self
            .inner
            .members
            .lock()
            .get_mut(channel)
            .is_some_and(|known| known.remove(&id).is_some());
        if changed {
            self.emit(channel);
        }
    }

    /// Laravel said no (its channel rule returned false), or the server
    /// refused the subscription: not joined, nobody shown.
    pub(crate) fn refused(&self, channel: &str) {
        crate::warn!(target: "elyra::presence", "joining {channel} was refused");
        self.inner.members.lock().remove(channel);
        self.emit(channel);
    }
}

/// A member's id, which Laravel sends as a number or a string.
pub(crate) fn user_id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn presence() -> Presence {
        Presence::new(vec!["customers.*".into(), "lobby".into()], None)
    }

    #[test]
    fn only_listed_valid_names_may_be_joined() {
        let p = presence();
        for ok in ["customers.42", "customers.a-b_c", "lobby"] {
            assert!(p.allows(ok), "{ok}");
        }
        for no in [
            "invoices.1",
            "lobbyX",
            "customers.4 2",
            "customers./x",
            "",
            &format!("customers.{}", "9".repeat(160)),
        ] {
            assert!(!p.allows(no), "{no}");
        }
        assert!(p
            .join("w1", "invoices.1")
            .unwrap_err()
            .contains("Backend::presence"));
    }

    #[test]
    fn a_channel_is_joined_once_and_left_by_the_last_window() {
        let p = presence();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(p.attach(tx, Some("7".into())).is_empty());
        p.join("w1", "customers.42").unwrap();
        p.join("w2", "customers.42").unwrap();
        p.join("w1", "customers.42").unwrap();
        assert_eq!(rx.try_recv().unwrap(), Command::Join("customers.42".into()));
        assert!(rx.try_recv().is_err(), "once");
        assert!(p.leave("w1", "customers.42"));
        assert!(!p.leave("w1", "customers.42"), "already left");
        assert!(rx.try_recv().is_err(), "w2 still wants it");
        assert!(p.leave("w2", "customers.42"));
        assert_eq!(
            rx.try_recv().unwrap(),
            Command::Leave("customers.42".into())
        );
        assert!(p.channels().is_empty());
    }

    #[test]
    fn members_come_and_go_and_the_connection_closing_forgets_them() {
        let p = presence();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        p.join("w1", "customers.42").unwrap();
        assert_eq!(p.attach(tx, Some("7".into())), ["customers.42"]);
        assert!(!p.state("customers.42").joined);

        p.subscribed(
            "customers.42",
            &json!({ "presence": { "ids": ["7", "9"], "hash": { "7": { "name": "Grete" }, "9": { "name": "Ada" } }, "count": 2 } }),
        );
        let state = p.state("customers.42");
        assert!(state.joined);
        assert_eq!(state.me.as_deref(), Some("7"));
        assert_eq!(
            state.members,
            [
                Member {
                    id: "7".into(),
                    info: json!({ "name": "Grete" })
                },
                Member {
                    id: "9".into(),
                    info: json!({ "name": "Ada" })
                },
            ]
        );
        p.added(
            "customers.42",
            &json!({ "user_id": 12, "user_info": { "name": "Linus" } }),
        );
        p.removed("customers.42", &json!({ "user_id": "9" }));
        let ids: Vec<String> = p
            .members("customers.42")
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["12", "7"]);

        // Not a channel this app wanted: ignored.
        p.subscribed("lobby", &json!({ "presence": { "hash": { "1": {} } } }));
        assert!(!p.state("lobby").joined);
        p.added("lobby", &json!({ "user_id": 1, "user_info": {} }));
        assert!(p.members("lobby").is_empty());

        p.detach();
        let state = p.state("customers.42");
        assert!(!state.joined);
        assert!(state.members.is_empty());
        // Still wanted: the next connection joins it again.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        assert_eq!(p.attach(tx, Some("7".into())), ["customers.42"]);
    }

    #[test]
    fn a_refused_join_shows_nobody() {
        let p = presence();
        p.join("w1", "customers.42").unwrap();
        p.subscribed(
            "customers.42",
            &json!({ "presence": { "hash": { "9": {} } } }),
        );
        p.refused("customers.42");
        assert!(!p.state("customers.42").joined);
        assert!(p.members("customers.42").is_empty());
    }

    #[test]
    fn leaving_forgets_the_members_and_a_join_without_a_connection_waits() {
        let p = presence();
        p.join("w1", "customers.42").unwrap();
        p.subscribed(
            "customers.42",
            &json!({ "presence": { "hash": { "9": {} } } }),
        );
        p.leave("w1", "customers.42");
        assert!(!p.state("customers.42").joined);
        // No connection: nothing to send, and it's wanted for the next one.
        p.join("w1", "customers.7").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        assert_eq!(p.attach(tx, None), ["customers.7"]);
    }

    #[test]
    fn ids_are_numbers_or_strings() {
        assert_eq!(user_id(&json!(42)).as_deref(), Some("42"));
        assert_eq!(user_id(&json!("u-1")).as_deref(), Some("u-1"));
        assert_eq!(user_id(&json!(null)), None);
        assert_eq!(wire("customers.42"), "presence-customers.42");
        assert_eq!(from_wire("presence-customers.42"), Some("customers.42"));
        assert_eq!(from_wire("private-customers"), None);
    }
}
