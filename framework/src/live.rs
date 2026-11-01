//! Live queries (RFC 0002): commands the frontend subscribes to.
//!
//! A `#[command(live)]` runs once when a window subscribes, with the tables it
//! reads recorded ([`elyra_db::live::track`]). From then on, a write to one of
//! those tables — reported by the database's change hub — or an
//! [`invalidate`](LiveRegistry::invalidate) of a key it
//! [depends on](crate::Ctx::depends_on) re-runs it, through the same middleware
//! pipeline, and pushes the new result to *that window only*:
//!
//! - **coalesced** — a burst of writes re-runs each subscription once per batch
//!   window (one frame by default), not once per row;
//! - **only when changed** — a result byte-identical to the last one sent isn't
//!   pushed;
//! - **read-only** — a re-run that writes fails (it would re-trigger itself);
//! - **bounded** — a window holds at most [`DEFAULT_LIMIT`] subscriptions.
//!
//! The wire: `POST elyra://localhost/__live/<command>` (body: the arguments,
//! as for `/__cmd/<command>`) answers `{ id, value }`; updates arrive on the
//! event channel `elyra:live:<id>` as `{ value }` or `{ error: { message,
//! kind } }`; `POST /__live-stop` (body: the id) ends it.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;

use crate::command::CommandRegistry;
use crate::container::{Ctx, WeakCtx};
use crate::error::{Error, Result};
use crate::event::EventBus;
use crate::middleware::Origin;

/// How long changes are gathered before the affected subscriptions re-run.
pub const DEFAULT_BATCH_WINDOW: Duration = Duration::from_millis(16);

/// The most live subscriptions one window may hold.
pub const DEFAULT_LIMIT: usize = 64;

/// The event channel a subscription's updates arrive on.
pub fn channel(id: &str) -> String {
    format!("elyra:live:{id}")
}

/// A subscription a window — or an MCP agent — opened.
struct Sub {
    client: String,
    command: String,
    args: Vec<u8>,
    origin: Origin,
    /// The window's label, as the shell knew it (RFC 0016): its re-runs
    /// carry it to the middleware.
    window: Option<String>,
    /// An agent's: told that the result changed, instead of a window's event.
    agent: Option<tokio::sync::mpsc::UnboundedSender<()>>,
    reads: BTreeSet<String>,
    /// The last thing pushed, so an identical result isn't pushed again.
    last: Last,
    /// When it was opened, in order: the newest has the highest. Read by
    /// the assistant's note of the window (`ai`) and the inspector.
    #[cfg_attr(not(any(feature = "ai", debug_assertions)), allow(dead_code))]
    opened: u64,
    /// How often a re-run's new result was pushed, and when last: for the
    /// inspector (RFC 0014).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pushes: u64,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pushed_at: Option<std::time::Instant>,
}

/// One of a window's live queries, as the inspector shows it (RFC 0014).
#[cfg(debug_assertions)]
#[derive(Debug, serde::Serialize)]
pub(crate) struct LiveRow {
    pub(crate) command: String,
    /// The arguments, in order, as JSON.
    pub(crate) arguments: serde_json::Value,
    /// The tables (and keys) its last run read: a write to one re-runs it.
    pub(crate) reads: Vec<String>,
    pub(crate) pushes: u64,
    /// How long ago its last push was, in milliseconds.
    pub(crate) pushed_ago_ms: Option<u64>,
}

#[derive(PartialEq)]
enum Last {
    Value(Vec<u8>),
    /// The message, and its kind when the error had one (`offline`, say).
    Error(String, Option<&'static str>),
}

#[derive(Default)]
struct Pending {
    keys: BTreeSet<String>,
    /// The change feed lagged: re-run everything.
    all: bool,
    scheduled: bool,
}

struct Inner {
    subs: Mutex<HashMap<String, Sub>>,
    pending: Mutex<Pending>,
    /// Set once the app is assembled: re-runs need the context and commands.
    runtime: OnceLock<(WeakCtx, Arc<CommandRegistry>)>,
    bus: EventBus,
    window: Duration,
    limit: usize,
    opened: std::sync::atomic::AtomicU64,
}

/// The app's live subscriptions. Bound in the container by `App` (with the
/// `database` feature); resolve it to [`invalidate`](Self::invalidate) a key.
#[derive(Clone)]
pub struct LiveRegistry {
    inner: Arc<Inner>,
}

/// A new subscription: its id, and the first result (MessagePack).
pub struct Subscribed {
    pub id: String,
    pub value: Vec<u8>,
}

impl LiveRegistry {
    pub(crate) fn new(bus: EventBus, window: Duration, limit: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                subs: Mutex::new(HashMap::new()),
                pending: Mutex::new(Pending::default()),
                runtime: OnceLock::new(),
                bus,
                window,
                limit,
                opened: std::sync::atomic::AtomicU64::new(0),
            }),
        }
    }

    /// Hand over what re-runs need, once the app is assembled.
    pub(crate) fn attach(&self, ctx: &Ctx, registry: Arc<CommandRegistry>) {
        let _ = self.inner.runtime.set((ctx.downgrade(), registry));
    }

    /// Follow a database's change hub. Needs a tokio runtime; without one
    /// (a synchronous test) subscriptions work but never update.
    pub(crate) fn watch(&self, db: &elyra_db::Database) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mut changes = db.changes().subscribe();
        let weak = Arc::downgrade(&self.inner);
        handle.spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                let next = changes.recv().await;
                let Some(inner) = weak.upgrade() else { return };
                let live = LiveRegistry { inner };
                match next {
                    Ok(key) => live.invalidate(&key),
                    Err(RecvError::Lagged(_)) => live.invalidate_all(),
                    Err(RecvError::Closed) => return,
                }
            }
        });
    }

    /// Subscribe `client` to `command(args)`: run it now, recording what it
    /// reads, and keep it to re-run. The caller has already passed the guard
    /// (capability, ability), exactly as for an ordinary call.
    pub async fn subscribe(&self, client: &str, command: &str, args: &[u8]) -> Result<Subscribed> {
        self.open(client, None, command, args, Origin::Frontend, None)
            .await
    }

    /// [`subscribe`](Self::subscribe) from window `window` (its label,
    /// RFC 0016): the runs carry it as `CommandRequest::window`.
    pub(crate) async fn subscribe_in(
        &self,
        client: &str,
        window: Option<String>,
        command: &str,
        args: &[u8],
    ) -> Result<Subscribed> {
        self.open(client, window, command, args, Origin::Frontend, None)
            .await
    }

    /// Subscribe an MCP agent (RFC 0003): like a window, but `changed` gets a
    /// `()` whenever the result changes, and the re-runs carry `origin`. It
    /// ends with [`unsubscribe`](Self::unsubscribe), or when the receiver is
    /// dropped. `client` keys the subscription limit.
    pub(crate) async fn subscribe_agent(
        &self,
        client: &str,
        command: &str,
        args: &[u8],
        origin: Origin,
        changed: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> Result<String> {
        self.open(client, None, command, args, origin, Some(changed))
            .await
            .map(|s| s.id)
    }

    async fn open(
        &self,
        client: &str,
        window: Option<String>,
        command: &str,
        args: &[u8],
        origin: Origin,
        agent: Option<tokio::sync::mpsc::UnboundedSender<()>>,
    ) -> Result<Subscribed> {
        let (_, registry) = self.runtime()?;
        if !registry.is_live(command) {
            return Err(Error::Command(format!(
                "`{command}` isn't a live command — declare it `#[command(live)]`"
            )));
        }
        let held = self
            .inner
            .subs
            .lock()
            .values()
            .filter(|s| s.client == client)
            .count();
        if held >= self.inner.limit {
            return Err(Error::Command(format!(
                "this window already holds {held} live subscriptions (the limit)"
            )));
        }
        let (result, reads) = self.run(command, args, &origin, window.clone()).await?;
        let value = result?;
        // The window polls for its updates; queue them even before it does.
        if agent.is_none() {
            self.inner.bus.register_client(client);
        }
        let id = crate::security::random_token();
        self.inner.subs.lock().insert(
            id.clone(),
            Sub {
                client: client.to_owned(),
                command: command.to_owned(),
                args: args.to_vec(),
                origin,
                window,
                agent,
                reads,
                last: Last::Value(value.clone()),
                opened: self
                    .inner
                    .opened
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                pushes: 0,
                pushed_at: None,
            },
        );
        Ok(Subscribed { id, value })
    }

    /// End a subscription. Only the window that opened it can: an id from
    /// another window is ignored.
    pub fn unsubscribe(&self, client: &str, id: &str) -> bool {
        let mut subs = self.inner.subs.lock();
        if subs.get(id).is_some_and(|s| s.client == client) {
            subs.remove(id);
            true
        } else {
            false
        }
    }

    /// What `client`'s window is showing: the commands its own frontend
    /// subscribed to, newest first, each with its arguments (MessagePack).
    /// An agent's subscriptions aren't a window's. For the assistant's note
    /// of the window (RFC 0013).
    #[cfg_attr(not(feature = "ai"), allow(dead_code))]
    pub(crate) fn showing(&self, client: &str) -> Vec<(String, Vec<u8>)> {
        let subs = self.inner.subs.lock();
        let mut shown: Vec<&Sub> = subs
            .values()
            .filter(|s| s.client == client && s.origin == Origin::Frontend)
            .collect();
        shown.sort_by_key(|s| std::cmp::Reverse(s.opened));
        shown
            .into_iter()
            .map(|s| (s.command.clone(), s.args.clone()))
            .collect()
    }

    /// `client`'s live queries for the inspector (RFC 0014), newest first:
    /// a window's own, not an agent's.
    #[cfg(debug_assertions)]
    pub(crate) fn inspect(&self, client: &str) -> Vec<LiveRow> {
        let subs = self.inner.subs.lock();
        let mut mine: Vec<&Sub> = subs
            .values()
            .filter(|s| s.client == client && s.origin == Origin::Frontend)
            .collect();
        mine.sort_by_key(|s| std::cmp::Reverse(s.opened));
        mine.into_iter()
            .map(|s| LiveRow {
                command: s.command.clone(),
                arguments: rmp_serde::from_slice(&s.args).unwrap_or(serde_json::Value::Null),
                reads: s.reads.iter().cloned().collect(),
                pushes: s.pushes,
                pushed_ago_ms: s.pushed_at.map(|at| at.elapsed().as_millis() as u64),
            })
            .collect()
    }

    /// How many subscriptions are open (across windows).
    pub fn len(&self) -> usize {
        self.inner.subs.lock().len()
    }

    /// Whether no subscription is open.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `key` changed: re-run, after the batch window, every subscription that
    /// read it. A table's key is its name; the database reports those itself.
    pub fn invalidate(&self, key: &str) {
        self.inner.pending.lock().keys.insert(key.to_owned());
        self.schedule();
    }

    fn invalidate_all(&self) {
        self.inner.pending.lock().all = true;
        self.schedule();
    }

    fn schedule(&self) {
        {
            let mut pending = self.inner.pending.lock();
            if pending.scheduled {
                return;
            }
            pending.scheduled = true;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.inner.pending.lock().scheduled = false;
            return;
        };
        let live = self.clone();
        let window = self.inner.window;
        // One flush at a time: two running together could finish out of order
        // and push an older result after a newer one. Changes that arrive
        // during a flush get the next round of this loop.
        handle.spawn(async move {
            loop {
                if !window.is_zero() {
                    tokio::time::sleep(window).await;
                }
                live.flush().await;
                let mut pending = live.inner.pending.lock();
                if pending.keys.is_empty() && !pending.all {
                    pending.scheduled = false;
                    return;
                }
            }
        });
    }

    /// Re-run what the pending changes affect, and push what changed.
    async fn flush(&self) {
        let (keys, all) = {
            let mut pending = self.inner.pending.lock();
            // `scheduled` stays set: the flush loop clears it when it's done.
            (
                std::mem::take(&mut pending.keys),
                std::mem::take(&mut pending.all),
            )
        };

        let bus = &self.inner.bus;
        #[allow(clippy::type_complexity)]
        let affected: Vec<(String, String, Vec<u8>, Origin, Option<String>)> = {
            let mut subs = self.inner.subs.lock();
            // A window (or an agent) that's gone takes its subscriptions with it.
            subs.retain(|_, s| match &s.agent {
                Some(changed) => !changed.is_closed(),
                None => bus.is_connected(&s.client),
            });
            subs.iter()
                .filter(|(_, s)| all || s.reads.iter().any(|r| keys.contains(r)))
                .map(|(id, s)| {
                    (
                        id.clone(),
                        s.command.clone(),
                        s.args.clone(),
                        s.origin.clone(),
                        s.window.clone(),
                    )
                })
                .collect()
        };

        for (id, command, args, origin, window) in affected {
            let Ok((result, reads)) = self.run(&command, &args, &origin, window).await else {
                return; // the app is gone
            };
            let next = match result {
                Ok(bytes) => Last::Value(bytes),
                Err(e) => Last::Error(e.to_string(), e.kind()),
            };
            let (client, agent) = {
                let mut subs = self.inner.subs.lock();
                let Some(sub) = subs.get_mut(&id) else {
                    continue; // unsubscribed meanwhile
                };
                // A failed run may not have reached every read; keep the old set.
                if matches!(next, Last::Value(_)) {
                    sub.reads = reads;
                }
                if sub.last == next {
                    continue;
                }
                (sub.client.clone(), sub.agent.clone())
            };
            match agent {
                Some(changed) => {
                    let _ = changed.send(());
                }
                None => {
                    if let Some(payload) = envelope(&next) {
                        bus.emit_encoded_to(&client, &channel(&id), payload);
                    }
                }
            }
            if let Some(sub) = self.inner.subs.lock().get_mut(&id) {
                sub.last = next;
                sub.pushes += 1;
                sub.pushed_at = Some(std::time::Instant::now());
            }
        }
    }

    /// Run `command` read-only, on its own task (as the shell does, so a panic
    /// is an error, not a lost reply), recording what it reads.
    async fn run(
        &self,
        command: &str,
        args: &[u8],
        origin: &Origin,
        window: Option<String>,
    ) -> Result<(Result<Vec<u8>>, BTreeSet<String>)> {
        let (ctx, registry) = self.runtime()?;
        let name = command.to_owned();
        let args = args.to_vec();
        let origin = origin.clone();
        let task = tokio::spawn(async move {
            let label = name.clone();
            elyra_db::live::track(
                Some(&label),
                registry.dispatch_in(ctx, &name, &args, origin, window),
            )
            .await
        });
        Ok(match task.await {
            Ok(done) => done,
            Err(e) => (
                Err(Error::Command(format!(
                    "live command `{command}` panicked: {e}"
                ))),
                BTreeSet::new(),
            ),
        })
    }

    fn runtime(&self) -> Result<(Ctx, Arc<CommandRegistry>)> {
        let (weak, registry) = self
            .inner
            .runtime
            .get()
            .ok_or_else(|| Error::Command("live queries aren't attached to an app".into()))?;
        let ctx = weak
            .upgrade()
            .ok_or_else(|| Error::Command("the app has shut down".into()))?;
        Ok((ctx, registry.clone()))
    }
}

impl Ctx {
    /// Live queries: the running command depends on `key` — a table it read
    /// with raw SQL, or any other key (`"settings:theme"`). Model-layer reads
    /// are recorded on their own. A no-op outside a live command.
    pub fn depends_on(&self, key: &str) {
        elyra_db::live::depends_on(key);
    }

    /// Live queries: `key` changed — re-run every live subscription that
    /// depends on it. Model-layer writes report their table on their own.
    pub fn invalidate(&self, key: &str) {
        if let Some(live) = self.try_get::<LiveRegistry>() {
            live.invalidate(key);
        }
    }
}

/// What a subscription's channel carries: `{ value }` or `{ error: { message,
/// kind } }`, MessagePack.
fn envelope(last: &Last) -> Option<Vec<u8>> {
    use rmpv::Value;
    let body = match last {
        Last::Value(bytes) => {
            let value = rmpv::decode::read_value(&mut bytes.as_slice()).ok()?;
            Value::Map(vec![(Value::from("value"), value)])
        }
        Last::Error(message, kind) => {
            let kind = match kind {
                Some(kind) => kind,
                None if crate::validation::is_validation_bag(message) => "validation",
                None => "command",
            };
            Value::Map(vec![(
                Value::from("error"),
                Value::Map(vec![
                    (Value::from("message"), Value::from(message.as_str())),
                    (Value::from("kind"), Value::from(kind)),
                ]),
            )])
        }
    };
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, &body).ok()?;
    Some(out)
}

/// `{ id, value }` — a subscription's first answer, MessagePack.
pub(crate) fn subscribed_body(subscribed: &Subscribed) -> Vec<u8> {
    use rmpv::Value;
    let value = rmpv::decode::read_value(&mut subscribed.value.as_slice()).unwrap_or(Value::Nil);
    let body = Value::Map(vec![
        (Value::from("id"), Value::from(subscribed.id.as_str())),
        (Value::from("value"), value),
    ]);
    let mut out = Vec::new();
    let _ = rmpv::encode::write_value(&mut out, &body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_in(last: &Last) -> String {
        let bytes = envelope(last).unwrap();
        let value = rmpv::decode::read_value(&mut bytes.as_slice()).unwrap();
        let rmpv::Value::Map(top) = value else {
            panic!()
        };
        let rmpv::Value::Map(error) = &top[0].1 else {
            panic!()
        };
        error
            .iter()
            .find(|(k, _)| k.as_str() == Some("kind"))
            .and_then(|(_, v)| v.as_str())
            .unwrap()
            .to_owned()
    }

    #[test]
    fn a_window_shows_its_own_frontend_queries_newest_first() {
        let live = LiveRegistry::new(EventBus::new(), Duration::from_millis(5), 10);
        let sub = |client: &str, command: &str, origin: Origin, opened: u64| Sub {
            client: client.into(),
            command: command.into(),
            args: vec![0x90],
            origin,
            window: None,
            agent: None,
            reads: BTreeSet::new(),
            last: Last::Value(Vec::new()),
            opened,
            pushes: 0,
            pushed_at: None,
        };
        {
            let mut subs = live.inner.subs.lock();
            subs.insert(
                "a".into(),
                sub("w1", "customers_index", Origin::Frontend, 0),
            );
            subs.insert("b".into(), sub("w1", "customers_show", Origin::Frontend, 2));
            subs.insert("c".into(), sub("w2", "invoices_index", Origin::Frontend, 1));
            // An MCP agent keyed by the same name isn't the window.
            let agent = Origin::Agent {
                client: "w1".into(),
            };
            subs.insert("d".into(), sub("w1", "audit_index", agent, 3));
        }
        let shown: Vec<String> = live.showing("w1").into_iter().map(|(c, _)| c).collect();
        assert_eq!(shown, ["customers_show", "customers_index"]);
        assert!(live.showing("w3").is_empty());
    }

    #[test]
    fn a_re_runs_error_keeps_its_kind() {
        assert_eq!(
            kind_in(&Last::Error("offline".into(), Some("offline"))),
            "offline"
        );
        assert_eq!(
            kind_in(&Last::Error(r#"{"a":["b"]}"#.into(), None)),
            "validation"
        );
        assert_eq!(kind_in(&Last::Error("x".into(), None)), "command");
    }
}
