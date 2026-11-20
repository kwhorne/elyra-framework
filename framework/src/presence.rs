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
    /// A whisper on a channel: `client-…` and its data (RFC 0042).
    Whisper(String, &'static str, Value),
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
    /// Who follows whom (RFC 0042).
    follow: Mutex<Follow>,
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
                follow: Mutex::new(Follow::default()),
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
        self.end_all_following();
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
            // Following ends when either leaves the channel (RFC 0042).
            self.gone(channel, &id);
        }
    }

    /// A member whispered on `channel` (`client-…`, with the `user_id` the
    /// server added): `client-editing` `{ editing }` marks them editing or
    /// not. Anything else, from someone the server didn't name or who isn't
    /// a member, is ignored.
    pub(crate) fn whispered(&self, channel: &str, event: &str, user: Option<String>, data: &Value) {
        if event.starts_with("client-follow") || event == ROUTE_EVENT {
            if let Some(user) = user.filter(|u| self.is_member(channel, u)) {
                self.follow_whisper(channel, event, &user, data);
            }
            return;
        }
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

// -- following a colleague (RFC 0042) ------------------------------------------

/// Where following is said to the windows.
pub const FOLLOW_CHANNEL: &str = "elyra:follow";

/// The leader's route, while someone follows them.
pub const ROUTE_EVENT: &str = "client-route";

/// How long a follow request waits for an answer.
const ASK_WAITS: Duration = Duration::from_secs(60);

/// How often the leader says its route again, for a follower who
/// reconnected.
const ROUTE_AGAIN: Duration = Duration::from_secs(30);

/// Who follows whom, as this app knows it.
#[derive(Default)]
struct Follow {
    /// This app's windows following someone: window → whom.
    following: HashMap<String, Following>,
    /// Who follows this user, per channel.
    followers: HashMap<String, std::collections::BTreeSet<String>>,
    /// Requests to follow this user, waiting for an answer: (channel, who).
    asked: HashMap<(String, String), Instant>,
    /// This user's route, as their focused window last said; `None` for one
    /// the app doesn't share.
    route: Option<String>,
    /// Saying the route again every 30 s while followed.
    repeating: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Following {
    pub channel: String,
    pub leader: String,
    /// The leader allowed it; until then it's asked.
    pub allowed: bool,
    #[serde(skip)]
    asked_at: Option<Instant>,
}

/// What's said on [`FOLLOW_CHANNEL`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FollowEvent {
    /// Someone asks to follow this user (to every window: the focused one
    /// asks the user).
    Asked {
        channel: String,
        from: String,
        info: Value,
    },
    /// This window follows `leader` now.
    Following { channel: String, leader: String },
    /// The leader said no, or didn't answer.
    Refused { channel: String, leader: String },
    /// Following ended, by either side or a member leaving.
    Stopped { channel: String, leader: String },
    /// The leader went here; `None`: somewhere the app doesn't share.
    Route { route: Option<String> },
    /// Who follows this user now.
    Followers { followers: Vec<Follower> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Follower {
    pub channel: String,
    pub id: String,
}

/// What a window knows of following, for the runtime's stores.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct FollowState {
    pub following: Option<Following>,
    pub followers: Vec<Follower>,
}

/// A route a follower's window may go to: one of the app's own (`/…`),
/// never a URL.
pub(crate) fn app_route(route: &str) -> Option<String> {
    let route = route.trim();
    let route = route.strip_prefix('#').unwrap_or(route);
    let ok = route.starts_with('/')
        && !route.starts_with("//")
        && !route.contains(['\\', '\n', '\r'])
        && route.len() <= 2000;
    ok.then(|| route.to_owned())
}

impl Presence {
    fn is_member(&self, channel: &str, user: &str) -> bool {
        self.inner
            .members
            .lock()
            .get(channel)
            .is_some_and(|m| m.contains_key(user))
    }

    fn whisper(&self, channel: &str, event: &'static str, data: Value) {
        self.send(Command::Whisper(channel.to_owned(), event, data));
    }

    fn tell_window(&self, window: &str, event: &FollowEvent) {
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit_to(window, FOLLOW_CHANNEL, event);
        }
    }

    fn tell_all(&self, event: &FollowEvent) {
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit(FOLLOW_CHANNEL, event);
        }
    }

    fn followers_list(&self) -> Vec<Follower> {
        self.inner
            .follow
            .lock()
            .followers
            .iter()
            .flat_map(|(channel, ids)| {
                ids.iter().map(|id| Follower {
                    channel: channel.clone(),
                    id: id.clone(),
                })
            })
            .collect()
    }

    fn tell_followers(&self) {
        let followers = self.followers_list();
        self.tell_all(&FollowEvent::Followers { followers });
    }

    /// What `window` knows of following now.
    pub fn follow_state(&self, window: &str) -> FollowState {
        // One lock at a time: a lock in a struct expression lives to its end.
        let following = self.inner.follow.lock().following.get(window).cloned();
        FollowState {
            following,
            followers: self.followers_list(),
        }
    }

    /// `window`, which joined `channel`, asks to follow `leader` there.
    pub fn follow(&self, window: &str, channel: &str, leader: &str) -> Result<FollowState, String> {
        let wants = self
            .inner
            .windows
            .lock()
            .get(channel)
            .is_some_and(|w| w.contains_key(window));
        if !wants || !self.joined(channel) {
            return Err(format!("join `{channel}` before following someone there"));
        }
        if !self.is_member(channel, leader) {
            return Err(format!("{leader} isn't in `{channel}`"));
        }
        if self.me().as_deref() == Some(leader) {
            return Err("you can't follow yourself".into());
        }
        self.unfollow(window);
        self.inner.follow.lock().following.insert(
            window.to_owned(),
            Following {
                channel: channel.to_owned(),
                leader: leader.to_owned(),
                allowed: false,
                asked_at: Some(Instant::now()),
            },
        );
        self.whisper(
            channel,
            "client-follow-ask",
            serde_json::json!({ "to": leader }),
        );
        self.refuse_unanswered(window.to_owned());
        Ok(self.follow_state(window))
    }

    /// No answer in a minute is a no.
    fn refuse_unanswered(&self, window: String) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            tokio::time::sleep(ASK_WAITS).await;
            let Some(inner) = weak.upgrade() else { return };
            let presence = Presence { inner };
            presence.expire_ask(&window, ASK_WAITS);
        });
    }

    fn expire_ask(&self, window: &str, after: Duration) {
        let expired = {
            let mut follow = self.inner.follow.lock();
            let old = follow
                .following
                .get(window)
                .filter(|f| !f.allowed && f.asked_at.is_some_and(|at| at.elapsed() >= after));
            old.is_some()
                .then(|| follow.following.remove(window))
                .flatten()
        };
        if let Some(f) = expired {
            self.tell_window(
                window,
                &FollowEvent::Refused {
                    channel: f.channel,
                    leader: f.leader,
                },
            );
        }
    }

    /// `window` stops following, as its Stop does, or its own navigation.
    pub fn unfollow(&self, window: &str) -> bool {
        let gone = self.inner.follow.lock().following.remove(window);
        match gone {
            Some(f) => {
                self.whisper(
                    &f.channel,
                    "client-follow-stop",
                    serde_json::json!({ "to": f.leader }),
                );
                self.tell_window(
                    window,
                    &FollowEvent::Stopped {
                        channel: f.channel,
                        leader: f.leader,
                    },
                );
                true
            }
            None => false,
        }
    }

    /// This user answers `from`'s request on `channel`.
    pub fn answer(&self, channel: &str, from: &str, allow: bool) -> bool {
        let asked = self
            .inner
            .follow
            .lock()
            .asked
            .remove(&(channel.to_owned(), from.to_owned()))
            .is_some();
        if !asked {
            return false;
        }
        if !allow {
            self.whisper(
                channel,
                "client-follow-no",
                serde_json::json!({ "to": from }),
            );
            return true;
        }
        let route = {
            let mut follow = self.inner.follow.lock();
            follow
                .followers
                .entry(channel.to_owned())
                .or_default()
                .insert(from.to_owned());
            follow.route.clone()
        };
        self.whisper(
            channel,
            "client-follow-yes",
            serde_json::json!({ "to": from }),
        );
        self.whisper(channel, ROUTE_EVENT, serde_json::json!({ "route": route }));
        self.tell_followers();
        self.keep_saying_route();
        true
    }

    /// This user stops everyone following them.
    pub fn stop_followers(&self) {
        let all = std::mem::take(&mut self.inner.follow.lock().followers);
        for (channel, ids) in all {
            for id in ids {
                self.whisper(
                    &channel,
                    "client-follow-stop",
                    serde_json::json!({ "to": id }),
                );
            }
        }
        self.tell_followers();
    }

    /// This user's focused window is at `route` now (`None`: a route the app
    /// doesn't share): said to every channel with followers.
    pub fn route(&self, route: Option<String>) {
        let route = route.and_then(|r| app_route(&r));
        let channels: Vec<String> = {
            let mut follow = self.inner.follow.lock();
            if follow.route == route {
                return;
            }
            follow.route = route.clone();
            follow.followers.keys().cloned().collect()
        };
        for channel in channels {
            self.whisper(&channel, ROUTE_EVENT, serde_json::json!({ "route": route }));
        }
    }

    fn keep_saying_route(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        {
            let mut follow = self.inner.follow.lock();
            if follow.repeating {
                return;
            }
            follow.repeating = true;
        }
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(ROUTE_AGAIN).await;
                let Some(inner) = weak.upgrade() else { return };
                let (route, channels) = {
                    let mut follow = inner.follow.lock();
                    if follow.followers.is_empty() {
                        follow.repeating = false;
                        return;
                    }
                    (
                        follow.route.clone(),
                        follow.followers.keys().cloned().collect::<Vec<_>>(),
                    )
                };
                let presence = Presence { inner };
                for channel in channels {
                    presence.whisper(&channel, ROUTE_EVENT, serde_json::json!({ "route": route }));
                }
            }
        });
    }

    /// A whisper about following, from `user`, a member the server named.
    fn follow_whisper(&self, channel: &str, event: &str, user: &str, data: &Value) {
        let to_me = data["to"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| user_id(&data["to"]))
            .is_some_and(|to| self.me().as_deref() == Some(to.as_str()));
        match event {
            "client-follow-ask" if to_me => {
                self.inner
                    .follow
                    .lock()
                    .asked
                    .insert((channel.to_owned(), user.to_owned()), Instant::now());
                let info = self
                    .inner
                    .members
                    .lock()
                    .get(channel)
                    .and_then(|m| m.get(user))
                    .map(|s| s.info.clone())
                    .unwrap_or(Value::Null);
                self.tell_all(&FollowEvent::Asked {
                    channel: channel.to_owned(),
                    from: user.to_owned(),
                    info,
                });
            }
            "client-follow-yes" | "client-follow-no" if to_me => {
                let windows: Vec<String> = {
                    let mut follow = self.inner.follow.lock();
                    let windows: Vec<String> = follow
                        .following
                        .iter()
                        .filter(|(_, f)| f.channel == channel && f.leader == user && !f.allowed)
                        .map(|(w, _)| w.clone())
                        .collect();
                    for w in &windows {
                        if event == "client-follow-yes" {
                            if let Some(f) = follow.following.get_mut(w) {
                                f.allowed = true;
                            }
                        } else {
                            follow.following.remove(w);
                        }
                    }
                    windows
                };
                for w in windows {
                    let (channel, leader) = (channel.to_owned(), user.to_owned());
                    let said = if event == "client-follow-yes" {
                        FollowEvent::Following { channel, leader }
                    } else {
                        FollowEvent::Refused { channel, leader }
                    };
                    self.tell_window(&w, &said);
                }
            }
            "client-follow-stop" if to_me => self.gone(channel, user),
            ROUTE_EVENT => {
                let route = data["route"].as_str().and_then(app_route);
                let windows: Vec<String> = self
                    .inner
                    .follow
                    .lock()
                    .following
                    .iter()
                    .filter(|(_, f)| f.channel == channel && f.leader == user && f.allowed)
                    .map(|(w, _)| w.clone())
                    .collect();
                for w in windows {
                    self.tell_window(
                        &w,
                        &FollowEvent::Route {
                            route: route.clone(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    /// `user` left `channel`, or stopped: they no longer follow this user,
    /// and this app's windows no longer follow them.
    fn gone(&self, channel: &str, user: &str) {
        let (was_follower, stopped) = {
            let mut follow = self.inner.follow.lock();
            let was = follow
                .followers
                .get_mut(channel)
                .is_some_and(|ids| ids.remove(user));
            follow.followers.retain(|_, ids| !ids.is_empty());
            follow.asked.remove(&(channel.to_owned(), user.to_owned()));
            let stopped: Vec<(String, Following)> = follow
                .following
                .iter()
                .filter(|(_, f)| f.channel == channel && f.leader == user)
                .map(|(w, f)| (w.clone(), f.clone()))
                .collect();
            for (w, _) in &stopped {
                follow.following.remove(w);
            }
            (was, stopped)
        };
        if was_follower {
            self.tell_followers();
        }
        for (w, f) in stopped {
            self.tell_window(
                &w,
                &FollowEvent::Stopped {
                    channel: f.channel,
                    leader: f.leader,
                },
            );
        }
    }

    /// The connection closed: nobody follows or is followed.
    fn end_all_following(&self) {
        let (followers, following) = {
            let mut follow = self.inner.follow.lock();
            let had = !follow.followers.is_empty();
            follow.followers.clear();
            follow.asked.clear();
            (had, std::mem::take(&mut follow.following))
        };
        if followers {
            self.tell_followers();
        }
        for (w, f) in following {
            self.tell_window(
                &w,
                &FollowEvent::Stopped {
                    channel: f.channel,
                    leader: f.leader,
                },
            );
        }
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

#[cfg(test)]
mod follow_tests {
    use super::*;
    use serde_json::json;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    /// Me (7) and Ada (9) in `lobby`, with this app's whispers caught.
    fn rig(bus: Option<EventBus>) -> (Presence, UnboundedReceiver<Command>) {
        let p = Presence::new(vec!["lobby".into()], bus);
        let (tx, rx) = unbounded_channel();
        p.join("w1", "lobby").unwrap();
        p.attach(tx, Some("7".into()));
        p.subscribed(
            "lobby",
            &json!({ "presence": { "hash": { "7": { "name": "Grete" }, "9": { "name": "Ada" } } } }),
        );
        (p, rx)
    }

    fn whispers(rx: &mut UnboundedReceiver<Command>) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        while let Ok(command) = rx.try_recv() {
            if let Command::Whisper(_, event, data) = command {
                out.push((event.to_owned(), data));
            }
        }
        out
    }

    async fn said(bus: &EventBus, window: &str) -> Vec<Value> {
        let bytes = tokio::time::timeout(Duration::from_secs(1), bus.next_batch_for(window))
            .await
            .unwrap_or_default();
        let batch: Vec<(String, Value)> = rmp_serde::from_slice(&bytes).unwrap_or_default();
        batch
            .into_iter()
            .filter(|(c, _)| c == FOLLOW_CHANNEL)
            .map(|(_, v)| v)
            .collect()
    }

    #[tokio::test]
    async fn a_follower_asks_is_allowed_follows_and_stops() {
        let bus = EventBus::new();
        bus.register_client("w1");
        let (p, mut rx) = rig(Some(bus.clone()));

        p.follow("w1", "lobby", "9").unwrap();
        assert_eq!(
            whispers(&mut rx),
            [("client-follow-ask".into(), json!({ "to": "9" }))]
        );
        assert!(!p.follow_state("w1").following.unwrap().allowed);

        // Not for me, or not from the one asked: ignored.
        p.whispered(
            "lobby",
            "client-follow-yes",
            Some("9".into()),
            &json!({ "to": "8" }),
        );
        assert!(!p.follow_state("w1").following.unwrap().allowed);

        p.whispered(
            "lobby",
            "client-follow-yes",
            Some("9".into()),
            &json!({ "to": "7" }),
        );
        assert!(p.follow_state("w1").following.unwrap().allowed);
        p.whispered(
            "lobby",
            ROUTE_EVENT,
            Some("9".into()),
            &json!({ "route": "/customers/42" }),
        );
        p.whispered(
            "lobby",
            ROUTE_EVENT,
            Some("9".into()),
            &json!({ "route": "https://evil.test" }),
        );
        let events = said(&bus, "w1").await;
        assert!(
            events.contains(&json!({ "kind": "following", "channel": "lobby", "leader": "9" })),
            "{events:?}"
        );
        assert!(
            events.contains(&json!({ "kind": "route", "route": "/customers/42" })),
            "{events:?}"
        );
        assert!(
            events.contains(&json!({ "kind": "route", "route": null })),
            "a URL isn't a route: {events:?}"
        );

        assert!(p.unfollow("w1"));
        assert_eq!(
            whispers(&mut rx),
            [("client-follow-stop".into(), json!({ "to": "9" }))]
        );
        assert!(p.follow_state("w1").following.is_none());
    }

    #[tokio::test]
    async fn a_leader_is_asked_and_its_route_goes_only_to_followers() {
        let (p, mut rx) = rig(None);
        // Nothing is said while nobody follows.
        p.route(Some("/customers/1".into()));
        assert!(whispers(&mut rx).is_empty());

        // From someone the server didn't name, or not a member: ignored.
        p.whispered("lobby", "client-follow-ask", None, &json!({ "to": "7" }));
        p.whispered(
            "lobby",
            "client-follow-ask",
            Some("12".into()),
            &json!({ "to": "7" }),
        );
        assert!(!p.answer("lobby", "12", true));

        p.whispered(
            "lobby",
            "client-follow-ask",
            Some("9".into()),
            &json!({ "to": "7" }),
        );
        assert!(p.answer("lobby", "9", true));
        assert_eq!(
            whispers(&mut rx),
            [
                ("client-follow-yes".into(), json!({ "to": "9" })),
                (ROUTE_EVENT.into(), json!({ "route": "/customers/1" })),
            ]
        );
        assert_eq!(
            p.follow_state("w1").followers,
            [Follower {
                channel: "lobby".into(),
                id: "9".into()
            }]
        );

        p.route(Some("/customers/2".into()));
        p.route(None); // a route the app keeps back
        assert_eq!(
            whispers(&mut rx),
            [
                (ROUTE_EVENT.into(), json!({ "route": "/customers/2" })),
                (ROUTE_EVENT.into(), json!({ "route": null })),
            ]
        );

        p.stop_followers();
        assert_eq!(
            whispers(&mut rx),
            [("client-follow-stop".into(), json!({ "to": "9" }))]
        );
        p.route(Some("/customers/3".into()));
        assert!(whispers(&mut rx).is_empty());
    }

    #[test]
    fn a_no_an_unanswered_ask_or_leaving_ends_it() {
        let (p, mut rx) = rig(None);
        p.follow("w1", "lobby", "9").unwrap();
        p.whispered(
            "lobby",
            "client-follow-no",
            Some("9".into()),
            &json!({ "to": 7 }),
        );
        assert!(p.follow_state("w1").following.is_none());

        p.follow("w1", "lobby", "9").unwrap();
        p.expire_ask("w1", Duration::ZERO);
        assert!(p.follow_state("w1").following.is_none());

        p.follow("w1", "lobby", "9").unwrap();
        p.whispered(
            "lobby",
            "client-follow-yes",
            Some("9".into()),
            &json!({ "to": "7" }),
        );
        p.removed("lobby", &json!({ "user_id": 9 }));
        assert!(p.follow_state("w1").following.is_none());

        // A follower leaving, and the connection closing.
        p.added("lobby", &json!({ "user_id": 9, "user_info": {} }));
        p.whispered(
            "lobby",
            "client-follow-ask",
            Some("9".into()),
            &json!({ "to": "7" }),
        );
        p.answer("lobby", "9", true);
        p.detach();
        assert!(p.follow_state("w1").followers.is_empty());
        let _ = whispers(&mut rx);

        // Not yourself, nor someone not there.
        let (p, _rx) = rig(None);
        assert!(p.follow("w1", "lobby", "7").is_err());
        assert!(p.follow("w1", "lobby", "12").is_err());
        assert!(
            p.follow("w2", "lobby", "9").is_err(),
            "a window that didn't join"
        );
    }

    #[test]
    fn only_the_app_s_own_routes() {
        assert_eq!(
            app_route("/customers/42?tab=a").as_deref(),
            Some("/customers/42?tab=a")
        );
        assert_eq!(
            app_route("#/customers/42").as_deref(),
            Some("/customers/42")
        );
        for bad in [
            "https://evil.test",
            "//evil.test",
            "javascript:x",
            "customers",
        ] {
            assert_eq!(app_route(bad), None, "{bad}");
        }
    }
}
