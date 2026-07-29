//! The event bus — Rust -> frontend push, batched per flush.
//!
//! This is Elyra's Broadcasting. Rust owns the state; the frontend is a
//! projection. Instead of one IPC round per change, emits accumulate in a queue
//! and are flushed as a single MessagePack **batch** to a long-poll connection
//! held open by `@elyra/runtime`.
//!
//! ## Transport
//! The frontend keeps one request open against `elyra://localhost/__events`.
//! When events are pending the shell responds with a batch and the frontend
//! immediately reconnects. No `evaluate_script`, no base64 — binary in, binary
//! out, same origin.
//!
//! ## Fan-out (one queue per client)
//! Every webview (window) identifies itself with a random client id sent as
//! `x-elyra-client-id` and gets **its own queue**: an emit is fanned out to all
//! connected clients. A single shared queue would let whichever window polled
//! first steal the batch, so the other windows silently lost events.
//!
//! Events emitted before *any* client has connected are held in a bootstrap
//! buffer and handed to the first client that polls, so nothing emitted during
//! startup is lost.
//!
//! ## Batching
//! With `batch_window == 0` the natural round-trip gap coalesces bursts: every
//! emit that lands between a response and the frontend's reconnect ships in one
//! batch. A non-zero window adds an explicit coalescing delay to force
//! frame-level batching under sustained, time-spaced streams.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::Notify;

use crate::error::Error;

/// How long a poll waits before returning an empty keep-alive batch.
const KEEPALIVE: Duration = Duration::from_secs(20);

/// A client that hasn't polled within this window is considered gone (window
/// closed / page navigated away) and its queue is dropped.
const STALE_AFTER: Duration = Duration::from_secs(120);

/// The client id used by callers that don't identify themselves (tests, and any
/// frontend older than the `x-elyra-client-id` header).
pub(crate) const DEFAULT_CLIENT: &str = "__default";

/// Upper bound on buffered (undelivered) events **per client**. If a frontend is
/// gone, reloading, or draining slowly, its queue can't grow without bound: once
/// full the oldest half is dropped. Generous enough that a healthy poll never
/// trips it.
const MAX_QUEUED: usize = 8192;

#[derive(Clone)]
struct QueuedEvent {
    channel: Arc<str>,
    /// Already MessagePack-encoded (named) payload, shared across clients.
    payload: Arc<[u8]>,
}

/// One connected frontend (window): its pending events and its wakeup.
struct Subscriber {
    queue: Mutex<Vec<QueuedEvent>>,
    notify: Notify,
    last_seen: Mutex<Instant>,
}

impl Subscriber {
    fn new() -> Self {
        Self {
            queue: Mutex::new(Vec::new()),
            notify: Notify::new(),
            last_seen: Mutex::new(Instant::now()),
        }
    }

    fn push(&self, event: QueuedEvent) {
        let mut queue = self.queue.lock();
        if queue.len() >= MAX_QUEUED {
            // Drop the oldest half (amortized O(1)) rather than the newest, so a
            // reconnecting frontend still gets the most recent state.
            let drop_to = queue.len() / 2;
            queue.drain(..drop_to);
        }
        queue.push(event);
    }

    fn is_empty(&self) -> bool {
        self.queue.lock().is_empty()
    }

    fn touch(&self) {
        *self.last_seen.lock() = Instant::now();
    }
}

struct Inner {
    subscribers: Mutex<HashMap<String, Arc<Subscriber>>>,
    /// Events emitted before any client connected; drained by the first one.
    bootstrap: Mutex<Vec<QueuedEvent>>,
    batch_window: Duration,
}

/// A cheap-to-clone handle to the application's event bus.
///
/// Bind-free: [`crate::App`] creates one, registers it in the container (so
/// commands resolve it via `ctx.get::<EventBus>()`), and hands a clone to the
/// shell's poll handler.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Inner>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Create a bus with no artificial batching delay.
    pub fn new() -> Self {
        Self::with_batch_window(Duration::ZERO)
    }

    /// Create a bus with an explicit coalescing window (e.g. `~8ms` for
    /// frame-level batching of high-frequency streams).
    pub fn with_batch_window(batch_window: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                subscribers: Mutex::new(HashMap::new()),
                bootstrap: Mutex::new(Vec::new()),
                batch_window,
            }),
        }
    }

    /// Emit an event on `channel` to **every** connected client. Callable from
    /// any thread; wakes each waiting poll.
    pub fn emit<T: Serialize>(&self, channel: &str, value: &T) -> crate::Result<()> {
        let payload = rmp_serde::to_vec_named(value).map_err(Error::encode)?;
        let event = QueuedEvent {
            channel: Arc::from(channel),
            payload: Arc::from(payload.into_boxed_slice()),
        };

        let subscribers = self.inner.subscribers.lock();
        if subscribers.is_empty() {
            // Nobody connected yet — hold it for the first client that polls.
            let mut bootstrap = self.inner.bootstrap.lock();
            if bootstrap.len() >= MAX_QUEUED {
                let drop_to = bootstrap.len() / 2;
                bootstrap.drain(..drop_to);
            }
            bootstrap.push(event);
            return Ok(());
        }

        for subscriber in subscribers.values() {
            subscriber.push(event.clone());
            subscriber.notify.notify_one();
        }
        Ok(())
    }

    /// Get (or create) the queue for `client`, dropping any client that stopped
    /// polling. The first client created inherits the bootstrap buffer.
    fn subscriber(&self, client: &str) -> Arc<Subscriber> {
        let mut subscribers = self.inner.subscribers.lock();

        // Reap closed windows so their queues stop accumulating events.
        subscribers.retain(|_, s| s.last_seen.lock().elapsed() < STALE_AFTER);

        if let Some(existing) = subscribers.get(client) {
            existing.touch();
            return existing.clone();
        }

        let subscriber = Arc::new(Subscriber::new());
        if subscribers.is_empty() {
            let pending = std::mem::take(&mut *self.inner.bootstrap.lock());
            if !pending.is_empty() {
                *subscriber.queue.lock() = pending;
            }
        }
        subscribers.insert(client.to_owned(), subscriber.clone());
        subscriber
    }

    /// Forget a client's queue (e.g. its window closed).
    pub fn disconnect(&self, client: &str) {
        self.inner.subscribers.lock().remove(client);
    }

    /// How many clients (windows) are currently connected.
    pub fn client_count(&self) -> usize {
        self.inner.subscribers.lock().len()
    }

    /// Register `client` up front, so emits are queued for it even before its
    /// first poll. Used by [`crate::testing::TestApp`].
    pub fn register_client(&self, client: &str) {
        let _ = self.subscriber(client);
    }

    /// Events waiting for `client` (0 when it isn't registered).
    pub fn pending_for(&self, client: &str) -> usize {
        self.inner
            .subscribers
            .lock()
            .get(client)
            .map(|s| s.queue.lock().len())
            .unwrap_or(0)
    }

    #[cfg(test)]
    fn queued_len(&self) -> usize {
        let bootstrap = self.inner.bootstrap.lock().len();
        let largest = self
            .inner
            .subscribers
            .lock()
            .values()
            .map(|s| s.queue.lock().len())
            .max()
            .unwrap_or(0);
        bootstrap.max(largest)
    }

    /// Await the next batch of events for the default client. Convenience for
    /// tests and single-window callers; the shell uses
    /// [`next_batch_for`](EventBus::next_batch_for).
    pub async fn next_batch(&self) -> Vec<u8> {
        self.next_batch_for(DEFAULT_CLIENT).await
    }

    /// Await the next batch of events for `client`, encoded as a MessagePack
    /// array of `[channel, value]` pairs. Used by the shell's `__events` handler,
    /// once per connected webview.
    ///
    /// Returns an empty batch after [`KEEPALIVE`] so the connection can refresh.
    pub async fn next_batch_for(&self, client: &str) -> Vec<u8> {
        let subscriber = self.subscriber(client);

        loop {
            // Register interest *before* checking, so an emit racing between the
            // check and the await cannot be lost (Notify stores one permit).
            let notified = subscriber.notify.notified();
            if !subscriber.is_empty() {
                break;
            }
            tokio::select! {
                _ = notified => {
                    // A stale permit can wake us with an empty queue; re-wait.
                    if subscriber.is_empty() {
                        continue;
                    }
                    break;
                }
                _ = tokio::time::sleep(KEEPALIVE) => {
                    subscriber.touch();
                    return encode_batch(&[]);
                }
            }
        }

        if !self.inner.batch_window.is_zero() {
            tokio::time::sleep(self.inner.batch_window).await;
        }

        let events = std::mem::take(&mut *subscriber.queue.lock());
        subscriber.touch();
        encode_batch(&events)
    }
}

/// Frame a slice of events as one MessagePack array of `[channel, value]`.
///
/// Each `payload` is already valid MessagePack, so it is appended verbatim —
/// no re-encoding, no `bin` wrapper, single decode on the JS side.
fn encode_batch(events: &[QueuedEvent]) -> Vec<u8> {
    let mut buf = Vec::new();
    rmp::encode::write_array_len(&mut buf, events.len() as u32).expect("vec write");
    for event in events {
        rmp::encode::write_array_len(&mut buf, 2).expect("vec write");
        rmp::encode::write_str(&mut buf, &event.channel).expect("vec write");
        buf.extend_from_slice(&event.payload);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_bounded_when_frontend_never_drains() {
        let bus = EventBus::new();
        for i in 0..(MAX_QUEUED * 2) {
            bus.emit("x", &(i as u32)).unwrap();
        }
        assert!(bus.queued_len() <= MAX_QUEUED);
        assert!(bus.queued_len() > 0);
    }

    #[tokio::test]
    async fn every_client_receives_every_event() {
        // Regression: with one shared queue, whichever window polled first stole
        // the batch and the other windows lost the events entirely.
        let bus = EventBus::new();

        // Both clients connect (an empty keep-alive poll registers them).
        let a = bus.clone();
        let b = bus.clone();
        let poll_a = tokio::spawn(async move { a.next_batch_for("win-a").await });
        let poll_b = tokio::spawn(async move { b.next_batch_for("win-b").await });

        // Wait until both are registered, then emit once.
        for _ in 0..200 {
            if bus.client_count() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(bus.client_count(), 2);
        bus.emit("tick", &7u32).unwrap();

        for buf in [poll_a.await.unwrap(), poll_b.await.unwrap()] {
            let batch: Vec<(String, u32)> = rmp_serde::from_slice(&buf).unwrap();
            assert_eq!(batch, vec![("tick".to_string(), 7)]);
        }
    }

    #[tokio::test]
    async fn first_client_inherits_events_emitted_before_it_connected() {
        let bus = EventBus::new();
        bus.emit("early", &1u32).unwrap();

        let buf = bus.next_batch_for("win-a").await;
        let batch: Vec<(String, u32)> = rmp_serde::from_slice(&buf).unwrap();
        assert_eq!(batch, vec![("early".to_string(), 1)]);

        // Once a client is connected, emits fan out to the connected clients — a
        // window that opens afterwards gets no replay of what it missed.
        bus.emit("later", &2u32).unwrap();
        let late =
            tokio::time::timeout(Duration::from_millis(50), bus.next_batch_for("win-b")).await;
        assert!(
            late.is_err(),
            "a new client must not receive earlier events"
        );

        // …while the already-connected client does get it.
        let buf = bus.next_batch_for("win-a").await;
        let batch: Vec<(String, u32)> = rmp_serde::from_slice(&buf).unwrap();
        assert_eq!(batch, vec![("later".to_string(), 2)]);
    }

    #[tokio::test]
    async fn disconnect_drops_the_queue() {
        let bus = EventBus::new();
        // Emit first so the poll returns immediately (no 20s keep-alive wait).
        bus.emit("x", &1u32).unwrap();
        let _ = bus.next_batch_for("gone").await;
        assert_eq!(bus.client_count(), 1);
        bus.disconnect("gone");
        assert_eq!(bus.client_count(), 0);
    }
}
