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
//! - A window says it's editing what the channel is about with
//!   `/__presence/editing` `{ channel, editing }`; the others in the channel
//!   hear it as Echo's `whisper('editing', { editing })` (a `client-editing`
//!   event), and a member who's editing has `editing: true`. It's what each
//!   says, not a lock: Laravel never sees a whisper.

use std::collections::{BTreeMap, HashMap, HashSet};
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

/// The whisper a member sends while it edits what the channel is about:
/// Echo's `whisper('editing', { editing: true })`.
pub const EDITING_EVENT: &str = "client-editing";

/// Someone in a channel: Laravel's `user_id`, and the `user_info` its
/// channel rule returned (a name, an avatar).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Member {
    pub id: String,
    pub info: Value,
    /// Whether they said they're editing it (a whisper; not a lock).
    pub editing: bool,
}

/// A member as the server and their whispers told it.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    info: Value,
    editing: bool,
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

/// A wanted channel, as the inspector lists it (RFC 0014's drawer).
#[cfg(debug_assertions)]
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct InspectedChannel {
    pub(crate) channel: String,
    pub(crate) joined: bool,
    pub(crate) members: Vec<Member>,
    /// How many windows want it.
    pub(crate) windows: usize,
    /// Whether the window asking is one of them.
    pub(crate) here: bool,
    /// Whether a window of this app is editing it.
    pub(crate) editing: bool,
}

/// What a window asks the open connection to do.
#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    Join(String),
    Leave(String),
    /// Tell the channel this app is editing (or no longer).
    Editing(String, bool),
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
    members: Mutex<HashMap<String, BTreeMap<String, Seen>>>,
    /// The windows editing each channel's record.
    editing: Mutex<HashMap<String, HashSet<String>>>,
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
                editing: Mutex::new(HashMap::new()),
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
        let stopped = self.set_editing(window, channel, false);
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
        // Said before leaving: the user may still be there in a browser tab.
        if stopped && self.joined(channel) {
            self.send(Command::Editing(channel.to_owned(), false));
        }
        if last {
            self.inner.members.lock().remove(channel);
            self.send(Command::Leave(channel.to_owned()));
        } else if stopped {
            self.emit(channel);
        }
        had
    }

    /// `window`, which joined `channel`, is editing what it's about, or no
    /// longer: the channel hears it when this app as a whole starts or
    /// stops. What's known of it now.
    pub fn editing(
        &self,
        window: &str,
        channel: &str,
        editing: bool,
    ) -> Result<PresenceState, String> {
        let wants = self
            .inner
            .windows
            .lock()
            .get(channel)
            .is_some_and(|w| w.contains_key(window));
        if !wants {
            return Err(format!("join `{channel}` before saying it's being edited"));
        }
        if self.set_editing(window, channel, editing) {
            // Not joined yet: said once the server answers (`subscribed`).
            if self.joined(channel) {
                self.send(Command::Editing(channel.to_owned(), editing));
            }
            self.emit(channel);
        }
        Ok(self.state(channel))
    }

    /// Whether a window of this app is editing `channel`'s record.
    pub fn editing_here(&self, channel: &str) -> bool {
        self.inner.editing.lock().contains_key(channel)
    }

    /// Mark `window` editing `channel` or not: whether the app as a whole
    /// started or stopped.
    fn set_editing(&self, window: &str, channel: &str, editing: bool) -> bool {
        let mut all = self.inner.editing.lock();
        if editing {
            let windows = all.entry(channel.to_owned()).or_default();
            windows.insert(window.to_owned()) && windows.len() == 1
        } else {
            let Some(windows) = all.get_mut(channel) else {
                return false;
            };
            let stopped = windows.remove(window) && windows.is_empty();
            if windows.is_empty() {
                all.remove(channel);
            }
            stopped
        }
    }

    /// Whether the server has said who's in `channel`.
    fn joined(&self, channel: &str) -> bool {
        self.inner.members.lock().contains_key(channel)
    }

    /// Who's in `channel` now, by id; empty when it isn't joined.
    pub fn members(&self, channel: &str) -> Vec<Member> {
        self.state(channel).members
    }

    /// The signed-in user's id, as the open connection learned it.
    pub fn me(&self) -> Option<String> {
        self.inner.me.read().clone()
    }

    /// The channels some window wants, in order.
    pub fn channels(&self) -> Vec<String> {
        let mut channels: Vec<String> = self.inner.windows.lock().keys().cloned().collect();
        channels.sort();
        channels
    }

    /// Every wanted channel, as the inspector lists it for `window`: who's
    /// there, how many windows want it, and whether `window` is one.
    #[cfg(debug_assertions)]
    pub(crate) fn inspect(&self, window: &str) -> Vec<InspectedChannel> {
        let wanted: Vec<(String, usize, bool)> = {
            let windows = self.inner.windows.lock();
            let mut rows: Vec<(String, usize, bool)> = windows
                .iter()
                .map(|(channel, wanting)| {
                    (channel.clone(), wanting.len(), wanting.contains_key(window))
                })
                .collect();
            rows.sort();
            rows
        };
        wanted
            .into_iter()
            .map(|(channel, windows, here)| {
                let state = self.state(&channel);
                InspectedChannel {
                    editing: self.editing_here(&channel),
                    channel,
                    joined: state.joined,
                    members: state.members,
                    windows,
                    here,
                }
            })
            .collect()
    }

    /// `channel` as a window sees it.
    pub fn state(&self, channel: &str) -> PresenceState {
        let me = self.me();
        // The user is editing when a window here is, or they said so
        // elsewhere (a browser tab).
        let here = self.editing_here(channel);
        let members = self.inner.members.lock();
        let known = members.get(channel);
        PresenceState {
            channel: channel.to_owned(),
            joined: known.is_some(),
            members: known
                .map(|m| {
                    m.iter()
                        .map(|(id, seen)| Member {
                            id: id.clone(),
                            info: seen.info.clone(),
                            editing: seen.editing || (here && me.as_deref() == Some(id)),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            me,
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
        let members: BTreeMap<String, Seen> = data["presence"]["hash"]
            .as_object()
            .map(|hash| {
                hash.iter()
                    .map(|(id, info)| {
                        let seen = Seen {
                            info: info.clone(),
                            editing: false,
                        };
                        (id.clone(), seen)
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.inner
            .members
            .lock()
            .insert(channel.to_owned(), members);
        // Whispers aren't kept: the others learn this app is editing now.
        if self.editing_here(channel) {
            self.send(Command::Editing(channel.to_owned(), true));
        }
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
            let seen = Seen {
                info: data["user_info"].clone(),
                editing: false,
            };
            known.insert(id, seen);
        }
        // Someone new: they learn this app is editing, as the others did.
        if self.editing_here(channel) {
            self.send(Command::Editing(channel.to_owned(), true));
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

    /// A member whispered on `channel` (`client-…`, with the `user_id` the
    /// server added): `client-editing` `{ editing }` marks them editing or
    /// not. Anything else, from someone the server didn't name or who isn't
    /// a member, is ignored.
    pub(crate) fn whispered(&self, channel: &str, event: &str, user: Option<String>, data: &Value) {
        let (EDITING_EVENT, Some(user), Some(editing)) = (event, user, data["editing"].as_bool())
        else {
            return;
        };
        let changed = self
            .inner
            .members
            .lock()
            .get_mut(channel)
            .and_then(|known| known.get_mut(&user))
            .is_some_and(|seen| std::mem::replace(&mut seen.editing, editing) != editing);
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
                    info: json!({ "name": "Grete" }),
                    editing: false,
                },
                Member {
                    id: "9".into(),
                    info: json!({ "name": "Ada" }),
                    editing: false,
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

    #[cfg(debug_assertions)]
    #[test]
    fn the_inspector_sees_every_wanted_channel_and_whose_it_is() {
        let p = presence();
        p.join("w1", "customers.42").unwrap();
        p.join("w2", "customers.42").unwrap();
        p.join("w2", "customers.7").unwrap();
        p.editing("w2", "customers.7", true).unwrap();
        p.subscribed(
            "customers.42",
            &json!({ "presence": { "hash": { "7": { "name": "Grete" } } } }),
        );
        let rows = p.inspect("w1");
        assert_eq!(
            rows,
            [
                InspectedChannel {
                    channel: "customers.42".into(),
                    joined: true,
                    members: vec![Member {
                        id: "7".into(),
                        info: json!({ "name": "Grete" }),
                        editing: false,
                    }],
                    windows: 2,
                    here: true,
                    editing: false,
                },
                InspectedChannel {
                    channel: "customers.7".into(),
                    joined: false,
                    members: vec![],
                    windows: 1,
                    here: false,
                    editing: true,
                },
            ]
        );
        assert!(Presence::new(vec![], None).inspect("w1").is_empty());
    }

    fn ids_editing(p: &Presence, channel: &str) -> Vec<String> {
        p.members(channel)
            .into_iter()
            .filter(|m| m.editing)
            .map(|m| m.id)
            .collect()
    }

    #[test]
    fn the_app_says_its_editing_when_its_first_window_starts_and_last_stops() {
        let p = presence();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        p.attach(tx, Some("7".into()));
        assert!(p
            .editing("w1", "customers.42", true)
            .unwrap_err()
            .contains("join `customers.42`"));
        assert!(!p.editing_here("customers.42"));
        p.join("w1", "customers.42").unwrap();
        p.join("w2", "customers.42").unwrap();
        assert_eq!(rx.try_recv().unwrap(), Command::Join("customers.42".into()));

        // Not joined yet: said once the server answers.
        p.editing("w1", "customers.42", true).unwrap();
        assert!(p.editing_here("customers.42"));
        assert!(rx.try_recv().is_err());
        p.subscribed(
            "customers.42",
            &json!({ "presence": { "hash": { "7": {}, "9": {} } } }),
        );
        assert_eq!(
            rx.try_recv().unwrap(),
            Command::Editing("customers.42".into(), true)
        );
        // The user shows as editing; nobody else does.
        assert_eq!(ids_editing(&p, "customers.42"), ["7"]);

        // A second window editing, the first stopping: nothing new to say.
        p.editing("w2", "customers.42", true).unwrap();
        p.editing("w1", "customers.42", false).unwrap();
        p.editing("w1", "customers.42", false).unwrap();
        assert!(rx.try_recv().is_err());
        // Someone new comes: they're told.
        p.added("customers.42", &json!({ "user_id": 12, "user_info": {} }));
        assert_eq!(
            rx.try_recv().unwrap(),
            Command::Editing("customers.42".into(), true)
        );
        // The last window editing stops: said once.
        p.editing("w2", "customers.42", false).unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            Command::Editing("customers.42".into(), false)
        );
        assert!(rx.try_recv().is_err());
        assert!(ids_editing(&p, "customers.42").is_empty());
        p.added("customers.42", &json!({ "user_id": 13, "user_info": {} }));
        assert!(rx.try_recv().is_err(), "not editing: nothing to tell");
    }

    #[test]
    fn leaving_while_editing_says_it_stopped_first() {
        let p = presence();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        p.attach(tx, Some("7".into()));
        p.join("w1", "customers.42").unwrap();
        p.join("w2", "customers.42").unwrap();
        p.subscribed(
            "customers.42",
            &json!({ "presence": { "hash": { "7": {} } } }),
        );
        p.editing("w1", "customers.42", true).unwrap();
        while rx.try_recv().is_ok() {}

        // w2 leaving changes nothing; w1 leaving stops the editing, then
        // the channel's left.
        p.leave("w2", "customers.42");
        assert!(rx.try_recv().is_err());
        p.leave("w1", "customers.42");
        assert_eq!(
            rx.try_recv().unwrap(),
            Command::Editing("customers.42".into(), false)
        );
        assert_eq!(
            rx.try_recv().unwrap(),
            Command::Leave("customers.42".into())
        );
        assert!(!p.editing_here("customers.42"));

        // A window that's still there and stops by leaving, with another
        // still in the channel: the others are told.
        p.join("w1", "customers.7").unwrap();
        p.join("w2", "customers.7").unwrap();
        p.subscribed(
            "customers.7",
            &json!({ "presence": { "hash": { "7": {} } } }),
        );
        p.editing("w1", "customers.7", true).unwrap();
        while rx.try_recv().is_ok() {}
        p.leave("w1", "customers.7");
        assert_eq!(
            rx.try_recv().unwrap(),
            Command::Editing("customers.7".into(), false)
        );
        assert!(rx.try_recv().is_err(), "w2 still wants it");

        // Not joined (no connection): nothing to say on leaving.
        p.join("w1", "customers.8").unwrap();
        p.editing("w1", "customers.8", true).unwrap();
        while rx.try_recv().is_ok() {}
        p.leave("w1", "customers.8");
        assert_eq!(rx.try_recv().unwrap(), Command::Leave("customers.8".into()));
    }

    #[test]
    fn a_members_whisper_marks_them_editing() {
        let p = presence();
        p.join("w1", "customers.42").unwrap();
        p.subscribed(
            "customers.42",
            &json!({ "presence": { "hash": { "7": {}, "9": { "name": "Ada" } } } }),
        );
        let editing = json!({ "editing": true });
        p.whispered("customers.42", EDITING_EVENT, Some("9".into()), &editing);
        assert_eq!(ids_editing(&p, "customers.42"), ["9"]);
        p.whispered(
            "customers.42",
            EDITING_EVENT,
            Some("9".into()),
            &json!({ "editing": false }),
        );
        assert!(ids_editing(&p, "customers.42").is_empty());

        // Not named by the server, not a member, another whisper, no flag:
        // ignored.
        p.whispered("customers.42", EDITING_EVENT, None, &editing);
        p.whispered("customers.42", EDITING_EVENT, Some("12".into()), &editing);
        p.whispered("customers.42", "client-typing", Some("9".into()), &editing);
        p.whispered(
            "customers.42",
            EDITING_EVENT,
            Some("9".into()),
            &json!({ "editing": "yes" }),
        );
        p.whispered("customers.7", EDITING_EVENT, Some("9".into()), &editing);
        assert!(ids_editing(&p, "customers.42").is_empty());
        assert!(p.members("customers.42").iter().all(|m| m.id != "12"));

        // Gone and back: not editing until they say so again.
        p.whispered("customers.42", EDITING_EVENT, Some("9".into()), &editing);
        p.removed("customers.42", &json!({ "user_id": 9 }));
        p.added("customers.42", &json!({ "user_id": 9, "user_info": {} }));
        assert!(ids_editing(&p, "customers.42").is_empty());
        // The connection dropping forgets it too.
        p.whispered("customers.42", EDITING_EVENT, Some("9".into()), &editing);
        p.detach();
        p.subscribed(
            "customers.42",
            &json!({ "presence": { "hash": { "9": {} } } }),
        );
        assert!(ids_editing(&p, "customers.42").is_empty());
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
