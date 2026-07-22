//! An ergonomic background **queue** facade — the desktop-side counterpart to
//! Laravel's `Queue::` / Askr's supervised queue workers. Same surface (`push`
//! a named job, register a handler), but scoped to a single process: jobs run
//! on a background task in-order, in-memory.
//!
//! **Not durable and not cross-process.** Jobs are lost on exit and there's no
//! separate worker fleet — that's Askr's domain on the server. Here it's for
//! offloading work off the UI thread (exports, uploads, cleanup) with the same
//! ergonomics you'd use on the Laravel side.
//!
//! Add [`QueueProvider`], register handlers in a provider's `boot` (or anywhere
//! with `ctx.get::<Queue>()`), and `push` from commands or the frontend
//! (`queue` in `@elyra/runtime`). Status is emitted on `elyra:queue`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::Mutex;

use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::event::EventBus;

type BoxFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type JobHandler = Arc<dyn Fn(Value) -> BoxFuture + Send + Sync>;

struct Job {
    name: String,
    payload: Value,
}

/// A single-process background job queue.
pub struct Queue {
    tx: UnboundedSender<Job>,
    rx: Mutex<Option<UnboundedReceiver<Job>>>,
    handlers: Arc<Mutex<HashMap<String, JobHandler>>>,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

impl Queue {
    /// A new queue (the worker starts via [`QueueProvider`]).
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: Mutex::new(Some(rx)),
            handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register the handler for a named job. Handlers are async and return
    /// `Result<(), String>` (an error is reported on `elyra:queue`).
    pub fn on<F, Fut>(&self, job: impl Into<String>, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        let boxed: JobHandler = Arc::new(move |payload| {
            let handler = handler.clone();
            Box::pin(async move { handler(payload).await })
        });
        self.handlers.lock().insert(job.into(), boxed);
    }

    /// Enqueue a job with a JSON payload. Returns immediately.
    pub fn push(&self, job: impl Into<String>, payload: impl Into<Value>) {
        let _ = self.tx.send(Job {
            name: job.into(),
            payload: payload.into(),
        });
    }

    /// Start the background worker (idempotent). Called by [`QueueProvider`].
    pub(crate) fn start(&self, bus: EventBus) {
        let Some(mut rx) = self.rx.lock().take() else {
            return; // already started
        };
        let handlers = self.handlers.clone();
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let handler = handlers.lock().get(&job.name).cloned();
                match handler {
                    Some(handler) => {
                        let _ = bus.emit(
                            "elyra:queue",
                            &json!({"job": job.name, "status": "processing"}),
                        );
                        match handler(job.payload).await {
                            Ok(()) => {
                                let _ = bus.emit(
                                    "elyra:queue",
                                    &json!({"job": job.name, "status": "processed"}),
                                );
                            }
                            Err(error) => {
                                let _ = bus.emit(
                                    "elyra:queue",
                                    &json!({"job": job.name, "status": "failed", "error": error}),
                                );
                            }
                        }
                    }
                    None => {
                        let _ = bus.emit(
                            "elyra:queue",
                            &json!({"job": job.name, "status": "unhandled"}),
                        );
                    }
                }
            }
        });
    }
}

/// Conformance to the shared [`substrate_core::Queue`] contract. The byte
/// payload is decoded as JSON (falling back to a JSON string).
impl substrate_core::Queue for Queue {
    fn push(&self, job: &str, payload: &[u8]) {
        let value = serde_json::from_slice::<Value>(payload)
            .unwrap_or_else(|_| Value::from(String::from_utf8_lossy(payload).into_owned()));
        Queue::push(self, job, value);
    }
}

/// A [`Provider`](crate::Provider) that binds a [`Queue`] and starts its worker.
///
/// ```no_run
/// use elyra::{App, Ctx, EventBus, Provider, Container};
/// use elyra::queue::{Queue, QueueProvider};
///
/// struct Jobs;
/// impl Provider for Jobs {
///     fn boot(&self, ctx: &Ctx) {
///         ctx.get::<Queue>().on("resize", |payload| async move {
///             // … do work …
///             Ok(())
///         });
///     }
/// }
///
/// App::new().provider(QueueProvider).provider(Jobs).run().unwrap();
/// ```
pub struct QueueProvider;

impl crate::Provider for QueueProvider {
    fn register(&self, container: &mut crate::Container) {
        container.bind(Queue::new());
    }

    fn boot(&self, ctx: &crate::Ctx) {
        let bus = ctx.get::<EventBus>().as_ref().clone();
        ctx.get::<Queue>().start(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn processes_registered_jobs() {
        let queue = Queue::new();
        let seen = Arc::new(Mutex::new(Vec::<i64>::new()));
        let sink = seen.clone();
        queue.on("add", move |payload| {
            let sink = sink.clone();
            async move {
                sink.lock().push(payload["n"].as_i64().unwrap_or(0));
                Ok(())
            }
        });
        queue.start(EventBus::new());
        queue.push("add", json!({"n": 7}));
        queue.push("add", json!({"n": 8}));
        // Give the worker a moment.
        for _ in 0..50 {
            if seen.lock().len() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(*seen.lock(), vec![7, 8]);
    }

    #[tokio::test]
    async fn start_is_idempotent() {
        let queue = Queue::new();
        queue.start(EventBus::new());
        queue.start(EventBus::new()); // no panic, no second worker
    }
}
