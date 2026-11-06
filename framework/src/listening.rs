//! Private channels a window listens to (RFC 0023): the private-channel
//! counterpart of presence. A window listens to `task.01JD…` while it shows
//! that run, and hears the events Laravel broadcasts on `private-task.01JD…`.
//!
//! ```ignore
//! Backend::new("https://felagi.example.com")
//!     .broadcasting(Broadcasting::pusher("app-key"))
//!     .listen(["task.*"])     // the private channels a window may listen to
//! ```
//!
//! - A window listens with `/__listen/join` `{ channel }` (the runtime's
//!   `listen(channel, handler)` does it) and stops with `/__listen/leave`.
//!   A channel is subscribed once however many windows listen, and
//!   unsubscribed when the last one stops, or stops polling (its window
//!   closed).
//! - Laravel decides who may listen, in `routes/channels.php`, through the
//!   backend's broadcasting auth, as for a private `follow(..)`. The app
//!   decides which channels it asks about: only names matching
//!   `Backend::listen(..)`.
//! - The events go to the windows listening, on `elyra:backend`, as a
//!   [`BackendEvent`] on the channel's
//!   name (`task.01JD…`), as a followed channel's do. Whether the server
//!   took the subscription goes to them on `elyra:listening`:
//!   `{ channel, joined }`. Without a connection (signed out, offline,
//!   unplugged) nothing is joined, and it's joined again when the
//!   connection is back.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::mpsc::UnboundedSender;

use crate::broadcasting::BackendEvent;
use crate::event::EventBus;

/// The channel whether a listened channel is joined reaches the windows on.
pub const CHANNEL: &str = "elyra:listening";

/// How often windows that stopped polling are let go of.
const SWEEP_EVERY: Duration = Duration::from_secs(30);

/// A window that started listening this recently isn't let go of for not
/// having polled yet.
const GRACE: Duration = Duration::from_secs(60);

/// A listened channel, as a window sees it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ListenState {
    /// The name the app uses (`task.01JD…`), not the wire's.
    pub channel: String,
    /// Whether the server took the subscription: its events reach the
    /// window. `false` while signed out, offline, or refused.
    pub joined: bool,
}

/// A listened channel, as the inspector lists it (RFC 0014's drawer).
#[cfg(debug_assertions)]
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct InspectedChannel {
    pub(crate) channel: String,
    pub(crate) joined: bool,
    /// How many windows listen.
    pub(crate) windows: usize,
    /// Whether the window asking is one of them.
    pub(crate) here: bool,
}

/// What a window asks the open connection to do.
#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    Join(String),
    Leave(String),
}

/// The private channels the windows listen to, and which the server took.
/// Shared by the shell's routes and the broadcasting connection.
#[derive(Clone)]
pub struct Listening {
    inner: Arc<Inner>,
}

struct Inner {
    patterns: Vec<String>,
    /// Each listened channel's windows, with when each started.
    windows: Mutex<HashMap<String, HashMap<String, Instant>>>,
    /// The channels the server took on the open connection.
    joined: Mutex<HashSet<String>>,
    /// The open connection's queue; `None` while there's none.
    session: Mutex<Option<UnboundedSender<Command>>>,
    bus: Option<EventBus>,
    sweeping: AtomicBool,
}

/// A listened channel's wire name: `private-` and the app's name.
pub(crate) fn wire(channel: &str) -> String {
    format!("private-{channel}")
}

/// The app's name for a channel on the wire, if it's a private channel.
pub(crate) fn from_wire(wire: &str) -> Option<&str> {
    wire.strip_prefix("private-")
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

impl Listening {
    pub(crate) fn new(patterns: Vec<String>, bus: Option<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                patterns,
                windows: Mutex::new(HashMap::new()),
                joined: Mutex::new(HashSet::new()),
                session: Mutex::new(None),
                bus,
                sweeping: AtomicBool::new(false),
            }),
        }
    }

    /// Whether a window may listen to `channel`: a valid name that one of
    /// `Backend::listen(..)`'s patterns covers.
    pub fn allows(&self, channel: &str) -> bool {
        valid(channel)
            && self
                .inner
                .patterns
                .iter()
                .any(|p| crate::security::ability_matches(p, channel))
    }

    /// `window` listens to `channel`: subscribed on the open connection if
    /// no other window was listening yet. What's known of it now.
    pub fn listen(&self, window: &str, channel: &str) -> Result<ListenState, String> {
        if !self.allows(channel) {
            return Err(format!(
                "`{channel}` isn't a private channel this app listens to (Backend::listen)"
            ));
        }
        let first = {
            let mut windows = self.inner.windows.lock();
            let listening = windows.entry(channel.to_owned()).or_default();
            let first = listening.is_empty();
            listening
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

    /// `window` stops listening to `channel`: unsubscribed when no window
    /// listens any more. Whether `window` was listening.
    pub fn unlisten(&self, window: &str, channel: &str) -> bool {
        let (had, last) = {
            let mut windows = self.inner.windows.lock();
            let Some(listening) = windows.get_mut(channel) else {
                return false;
            };
            let had = listening.remove(window).is_some();
            let last = listening.is_empty();
            if last {
                windows.remove(channel);
            }
            (had, last)
        };
        if last {
            self.inner.joined.lock().remove(channel);
            self.send(Command::Leave(channel.to_owned()));
        }
        had
    }

    /// Whether some window listens to `channel`.
    pub fn wanted(&self, channel: &str) -> bool {
        self.inner.windows.lock().contains_key(channel)
    }

    /// Whether the server took `channel` on the open connection.
    pub fn joined(&self, channel: &str) -> bool {
        self.inner.joined.lock().contains(channel)
    }

    /// The channels some window listens to, in order.
    pub fn channels(&self) -> Vec<String> {
        let mut channels: Vec<String> = self.inner.windows.lock().keys().cloned().collect();
        channels.sort();
        channels
    }

    /// `channel` as a window sees it.
    pub fn state(&self, channel: &str) -> ListenState {
        ListenState {
            channel: channel.to_owned(),
            joined: self.joined(channel),
        }
    }

    /// Every listened channel, as the inspector lists it for `window`.
    #[cfg(debug_assertions)]
    pub(crate) fn inspect(&self, window: &str) -> Vec<InspectedChannel> {
        let mut rows: Vec<InspectedChannel> = self
            .inner
            .windows
            .lock()
            .iter()
            .map(|(channel, listening)| InspectedChannel {
                channel: channel.clone(),
                joined: false,
                windows: listening.len(),
                here: listening.contains_key(window),
            })
            .collect();
        for row in &mut rows {
            row.joined = self.joined(&row.channel);
        }
        rows.sort_by(|a, b| a.channel.cmp(&b.channel));
        rows
    }

    /// Let go of the windows that stopped polling — their windows closed —
    /// unless they started within `grace`; channels no window listens to
    /// any more are unsubscribed. Runs every 30 s while anything is
    /// listened to.
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
            .flat_map(|(channel, listening)| {
                listening
                    .iter()
                    .filter(|(window, since)| since.elapsed() >= grace && !bus.is_connected(window))
                    .map(|(window, _)| (channel.clone(), window.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (channel, window) in gone {
            self.unlisten(&window, &channel);
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
                let listening = Listening { inner };
                listening.sweep(GRACE);
                if listening.inner.windows.lock().is_empty() {
                    listening.inner.sweeping.store(false, Ordering::SeqCst);
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

    /// The windows listening to `channel`.
    fn windows_of(&self, channel: &str) -> Vec<String> {
        self.inner
            .windows
            .lock()
            .get(channel)
            .map(|w| w.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Tell the windows listening to `channel` whether it's joined.
    fn emit(&self, channel: &str) {
        let Some(bus) = &self.inner.bus else {
            return;
        };
        let state = self.state(channel);
        for window in self.windows_of(channel) {
            let _ = bus.emit_to(&window, CHANNEL, &state);
        }
    }

    // -- the connection's side ------------------------------------------------

    /// A connection opened: it takes the windows' listens from now on. The
    /// channels to subscribe to on it now.
    pub(crate) fn attach(&self, session: UnboundedSender<Command>) -> Vec<String> {
        *self.inner.session.lock() = Some(session);
        self.channels()
    }

    /// The connection closed: nothing is joined until the next one says so.
    pub(crate) fn detach(&self) {
        *self.inner.session.lock() = None;
        let known: Vec<String> = self.inner.joined.lock().drain().collect();
        for channel in known {
            self.emit(&channel);
        }
    }

    /// The server took the subscription (`pusher_internal:subscription_succeeded`).
    pub(crate) fn subscribed(&self, channel: &str) {
        if !self.wanted(channel) {
            return;
        }
        if self.inner.joined.lock().insert(channel.to_owned()) {
            self.emit(channel);
        }
    }

    /// Laravel said no (a `403` from its channel rule), or the server
    /// refused the subscription: not joined, and the connection goes on.
    pub(crate) fn refused(&self, channel: &str) {
        crate::warn!(target: "elyra::listening", "listening to {channel} was refused");
        self.inner.joined.lock().remove(channel);
        self.emit(channel);
    }

    /// An event on a listened channel: to the windows listening, on
    /// `elyra:backend`. Whether anyone was.
    pub(crate) fn heard(&self, event: &BackendEvent) -> bool {
        let windows = self.windows_of(&event.channel);
        if windows.is_empty() {
            return false;
        }
        crate::debug!(
            target: "elyra::listening",
            "{} on {}",
            event.event,
            event.channel
        );
        if let Some(bus) = &self.inner.bus {
            for window in windows {
                let _ = bus.emit_to(&window, crate::broadcasting::CHANNEL, event);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn listening() -> Listening {
        Listening::new(vec!["task.*".into(), "lobby".into()], None)
    }

    #[test]
    fn only_listed_valid_names_may_be_listened_to() {
        let l = listening();
        for ok in ["task.01JD8X3Q", "task.a-b_c", "lobby"] {
            assert!(l.allows(ok), "{ok}");
        }
        for no in [
            "invoices.1",
            "task",
            "lobbyX",
            "task.4 2",
            "task./x",
            "private-task.1",
            "",
            &format!("task.{}", "9".repeat(160)),
        ] {
            assert!(!l.allows(no), "{no}");
        }
        assert!(l
            .listen("w1", "invoices.1")
            .unwrap_err()
            .contains("Backend::listen"));
        assert!(l.channels().is_empty());
    }

    #[test]
    fn a_channel_is_subscribed_once_and_left_by_the_last_window() {
        let l = listening();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(l.attach(tx).is_empty());
        l.listen("w1", "task.1").unwrap();
        l.listen("w2", "task.1").unwrap();
        l.listen("w1", "task.1").unwrap();
        assert_eq!(rx.try_recv().unwrap(), Command::Join("task.1".into()));
        assert!(rx.try_recv().is_err(), "once");
        assert!(l.unlisten("w1", "task.1"));
        assert!(!l.unlisten("w1", "task.1"), "already stopped");
        assert!(rx.try_recv().is_err(), "w2 still listens");
        assert!(l.unlisten("w2", "task.1"));
        assert_eq!(rx.try_recv().unwrap(), Command::Leave("task.1".into()));
        assert!(l.channels().is_empty());
        assert!(!l.unlisten("w2", "task.1"));
    }

    #[test]
    fn joined_comes_from_the_server_and_goes_with_the_connection() {
        let l = listening();
        l.listen("w1", "task.1").unwrap();
        // Without a connection: wanted, not joined, and waiting for one.
        assert!(!l.state("task.1").joined);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        assert_eq!(l.attach(tx), ["task.1"]);
        l.subscribed("task.1");
        assert!(l.state("task.1").joined);

        // Not a channel a window listens to (a follow's): ignored.
        l.subscribed("customers");
        assert!(!l.joined("customers"));

        l.detach();
        assert!(!l.state("task.1").joined);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        assert_eq!(l.attach(tx), ["task.1"], "joined again on the next one");

        l.subscribed("task.1");
        l.refused("task.1");
        assert!(!l.joined("task.1"));
        assert!(
            l.wanted("task.1"),
            "still wanted: tried on the next connection"
        );
    }

    #[test]
    fn stopping_forgets_it_was_joined() {
        let l = listening();
        l.listen("w1", "task.1").unwrap();
        l.subscribed("task.1");
        l.unlisten("w1", "task.1");
        assert!(!l.joined("task.1"));
    }

    #[test]
    fn an_event_reaches_only_the_windows_listening() {
        let bus = EventBus::new();
        for w in ["w1", "w2", "w3"] {
            bus.register_client(w);
        }
        let l = Listening::new(vec!["task.*".into()], Some(bus.clone()));
        l.listen("w1", "task.1").unwrap();
        l.listen("w2", "task.1").unwrap();
        l.listen("w3", "task.2").unwrap();
        let event = BackendEvent {
            channel: "task.1".into(),
            event: "RunOutput".into(),
            data: json!({ "chunk": "hello" }),
        };
        assert!(l.heard(&event));
        assert_eq!(bus.pending_for("w1"), 1);
        assert_eq!(bus.pending_for("w2"), 1);
        assert_eq!(bus.pending_for("w3"), 0);
        let nobody = BackendEvent {
            channel: "task.9".into(),
            ..event
        };
        assert!(!l.heard(&nobody));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_inspector_sees_every_listened_channel_and_whose_it_is() {
        let l = listening();
        l.listen("w1", "task.1").unwrap();
        l.listen("w2", "task.1").unwrap();
        l.listen("w2", "task.2").unwrap();
        l.subscribed("task.1");
        assert_eq!(
            l.inspect("w1"),
            [
                InspectedChannel {
                    channel: "task.1".into(),
                    joined: true,
                    windows: 2,
                    here: true,
                },
                InspectedChannel {
                    channel: "task.2".into(),
                    joined: false,
                    windows: 1,
                    here: false,
                },
            ]
        );
        assert!(Listening::new(vec![], None).inspect("w1").is_empty());
    }

    #[test]
    fn names_on_the_wire() {
        assert_eq!(wire("task.1"), "private-task.1");
        assert_eq!(from_wire("private-task.1"), Some("task.1"));
        assert_eq!(from_wire("presence-task.1"), None);
    }
}
