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
//! ## Batching
//! With `batch_window == 0` the natural round-trip gap coalesces bursts: every
//! emit that lands between a response and the frontend's reconnect ships in one
//! batch. A non-zero window adds an explicit coalescing delay to force
//! frame-level batching under sustained, time-spaced streams.

use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Notify;

use crate::error::Error;

/// How long a poll waits before returning an empty keep-alive batch.
const KEEPALIVE: Duration = Duration::from_secs(20);

/// Upper bound on buffered (undelivered) events. If the frontend is gone,
/// reloading, or draining slowly, the queue can't grow without bound: once full
/// the oldest half is dropped. Generous enough that a healthy poll never trips it.
const MAX_QUEUED: usize = 8192;

struct QueuedEvent {
    channel: String,
    /// Already MessagePack-encoded (named) payload.
    payload: Vec<u8>,
}

struct Inner {
    queue: Mutex<Vec<QueuedEvent>>,
    notify: Notify,
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
                queue: Mutex::new(Vec::new()),
                notify: Notify::new(),
                batch_window,
            }),
        }
    }

    /// Emit an event on `channel`. Callable from any thread; wakes a waiting poll.
    pub fn emit<T: Serialize>(&self, channel: &str, value: &T) -> crate::Result<()> {
        let payload = rmp_serde::to_vec_named(value).map_err(Error::encode)?;
        {
            let mut queue = self.inner.queue.lock();
            if queue.len() >= MAX_QUEUED {
                // Drop the oldest half (amortized O(1)) rather than the newest,
                // so a reconnecting frontend still gets the most recent state.
                let drop_to = queue.len() / 2;
                queue.drain(..drop_to);
            }
            queue.push(QueuedEvent {
                channel: channel.to_owned(),
                payload,
            });
        }
        self.inner.notify.notify_one();
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.inner.queue.lock().is_empty()
    }

    #[cfg(test)]
    fn queued_len(&self) -> usize {
        self.inner.queue.lock().len()
    }

    /// Await the next batch of events, encoded as a MessagePack array of
    /// `[channel, value]` pairs. Used by the shell's `__events` handler.
    ///
    /// Returns an empty batch after [`KEEPALIVE`] so the connection can refresh.
    pub async fn next_batch(&self) -> Vec<u8> {
        loop {
            // Register interest *before* checking, so an emit racing between the
            // check and the await cannot be lost (Notify stores one permit).
            let notified = self.inner.notify.notified();
            if !self.is_empty() {
                break;
            }
            tokio::select! {
                _ = notified => {
                    // A stale permit can wake us with an empty queue; re-wait.
                    if self.is_empty() {
                        continue;
                    }
                    break;
                }
                _ = tokio::time::sleep(KEEPALIVE) => return encode_batch(&[]),
            }
        }

        if !self.inner.batch_window.is_zero() {
            tokio::time::sleep(self.inner.batch_window).await;
        }

        let events = std::mem::take(&mut *self.inner.queue.lock());
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
}
