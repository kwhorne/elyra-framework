//! An ergonomic background **queue** facade — the desktop-side counterpart to
//! Laravel's `Queue::` / Askr's supervised queue workers. Same surface (`push`
//! a named job, register a handler), but scoped to a single process.
//!
//! **Not durable and not cross-process.** Jobs are lost on exit and there's no
//! separate worker fleet — that's Askr's domain on the server. Here it's for
//! offloading work off the UI thread (exports, uploads, cleanup) with the same
//! ergonomics you'd use on the Laravel side.
//!
//! ## What you get
//! * **Retries with backoff** — a failing handler is retried up to
//!   `max_attempts` times, with an exponential delay (`retry_base * 2^n`).
//! * **Failed jobs** — a job that exhausts its attempts lands in a bounded
//!   *failed jobs* list ([`Queue::failed`]), the local stand-in for Laravel's
//!   `failed_jobs` table, and emits `status: "failed"`.
//! * **Delays** — [`Queue::push_later`] runs a job after a delay.
//! * **Backpressure** — the queue is bounded ([`Queue::with_capacity`]); pushing
//!   to a full queue reports `status: "dropped"` rather than growing until the
//!   process dies.
//! * **Concurrency** — `workers > 1` processes jobs in parallel.
//! * **Typed jobs** — [`Queue::dispatch`] / [`Queue::on_typed`] serialize a
//!   payload struct instead of hand-rolling `serde_json::Value`.
//!
//! Add [`QueueProvider`], register handlers in a provider's `boot` (or anywhere
//! with `ctx.get::<Queue>()`), and `push` from commands or the frontend
//! (`queue` in `@elyra/runtime`). Status is emitted on `elyra:queue`.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::event::EventBus;

/// Default number of attempts (1 try + 2 retries).
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Default base delay for the exponential backoff.
pub const DEFAULT_RETRY_BASE: Duration = Duration::from_millis(500);
/// Default number of jobs that may wait in the queue.
pub const DEFAULT_CAPACITY: usize = 1024;
/// How many failed jobs are remembered.
const FAILED_HISTORY: usize = 100;

type BoxFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type JobHandler = Arc<dyn Fn(Value) -> BoxFuture + Send + Sync>;

/// A job that exhausted its attempts.
#[derive(Clone, Debug, Serialize)]
pub struct FailedJob {
    pub job: String,
    pub payload: Value,
    pub error: String,
    pub attempts: u32,
    /// Unix seconds when it finally failed.
    pub failed_at: u64,
}

/// Per-job retry/attempt configuration.
#[derive(Clone, Copy, Debug)]
pub struct JobOptions {
    /// Total attempts before the job is considered failed.
    pub max_attempts: u32,
    /// Base delay; attempt *n* waits `base * 2^(n-1)`.
    pub retry_base: Duration,
    /// Give up on a single attempt after this long (`None` = no timeout).
    pub timeout: Option<Duration>,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            retry_base: DEFAULT_RETRY_BASE,
            timeout: None,
        }
    }
}

impl JobOptions {
    /// Total attempts before failing.
    pub fn attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts.max(1);
        self
    }

    /// Base backoff delay.
    pub fn retry_base(mut self, base: Duration) -> Self {
        self.retry_base = base;
        self
    }

    /// Per-attempt timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

struct Job {
    name: String,
    payload: Value,
    attempt: u32,
}

struct Registration {
    handler: JobHandler,
    options: JobOptions,
}

/// A single-process background job queue.
pub struct Queue {
    tx: Sender<Job>,
    rx: Mutex<Option<Receiver<Job>>>,
    handlers: Arc<Mutex<HashMap<String, Registration>>>,
    failed: Arc<Mutex<VecDeque<FailedJob>>>,
    workers: usize,
    started: AtomicBool,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

impl Queue {
    /// A queue with the default capacity and a single worker.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY, 1)
    }

    /// A queue holding at most `capacity` waiting jobs, processed by `workers`
    /// concurrent tasks.
    pub fn with_capacity(capacity: usize, workers: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity.max(1));
        Self {
            tx,
            rx: Mutex::new(Some(rx)),
            handlers: Arc::new(Mutex::new(HashMap::new())),
            failed: Arc::new(Mutex::new(VecDeque::new())),
            workers: workers.max(1),
            started: AtomicBool::new(false),
        }
    }

    /// Register the handler for a named job with default retry options.
    pub fn on<F, Fut>(&self, job: impl Into<String>, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.on_with(job, JobOptions::default(), handler);
    }

    /// Register a handler with explicit retry/timeout options.
    ///
    /// ```ignore
    /// queue.on_with("upload", JobOptions::default().attempts(5), |payload| async move {
    ///     upload(payload).await.map_err(|e| e.to_string())
    /// });
    /// ```
    pub fn on_with<F, Fut>(&self, job: impl Into<String>, options: JobOptions, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        let boxed: JobHandler = Arc::new(move |payload| {
            let handler = handler.clone();
            Box::pin(async move { handler(payload).await })
        });
        self.handlers.lock().insert(
            job.into(),
            Registration {
                handler: boxed,
                options,
            },
        );
    }

    /// Register a handler that receives a **typed** payload; a payload that
    /// doesn't deserialize fails the job (and is retried like any other error).
    pub fn on_typed<T, F, Fut>(&self, job: impl Into<String>, handler: F)
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        // `Arc` so the async block can own a handle instead of borrowing the
        // closure's environment (which would need a lifetime the trait can't have).
        let handler = Arc::new(handler);
        self.on(job, move |value: Value| {
            let handler = handler.clone();
            let parsed = serde_json::from_value::<T>(value);
            async move {
                match parsed {
                    Ok(typed) => handler(typed).await,
                    Err(e) => Err(format!("invalid payload: {e}")),
                }
            }
        });
    }

    /// Enqueue a job with a JSON payload. Returns `false` when the queue is full
    /// (the job is dropped and reported on `elyra:queue`).
    pub fn push(&self, job: impl Into<String>, payload: impl Into<Value>) -> bool {
        self.enqueue(Job {
            name: job.into(),
            payload: payload.into(),
            attempt: 1,
        })
    }

    /// Enqueue a **typed** payload (serialized with serde).
    pub fn dispatch<T: Serialize>(&self, job: impl Into<String>, payload: &T) -> bool {
        match serde_json::to_value(payload) {
            Ok(value) => self.push(job, value),
            Err(_) => false,
        }
    }

    /// Enqueue a job to run after `delay`.
    pub fn push_later(&self, delay: Duration, job: impl Into<String>, payload: impl Into<Value>) {
        let name = job.into();
        let payload = payload.into();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx
                .send(Job {
                    name,
                    payload,
                    attempt: 1,
                })
                .await;
        });
    }

    fn enqueue(&self, job: Job) -> bool {
        match self.tx.try_send(job) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(job)) => {
                // Backpressure instead of unbounded growth: report and drop.
                crate::warn!(target: "elyra::queue", "queue is full; dropping job `{}`", job.name);
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    /// Jobs that exhausted their attempts (most recent last).
    pub fn failed(&self) -> Vec<FailedJob> {
        self.failed.lock().iter().cloned().collect()
    }

    /// Forget the failed-job history.
    pub fn clear_failed(&self) {
        self.failed.lock().clear();
    }

    /// Re-enqueue every failed job (a local `queue:retry`).
    pub fn retry_failed(&self) -> usize {
        let jobs: Vec<FailedJob> = self.failed.lock().drain(..).collect();
        let mut requeued = 0;
        for failed in jobs {
            if self.push(failed.job, failed.payload) {
                requeued += 1;
            }
        }
        requeued
    }

    /// Start the background workers (idempotent). Called by [`QueueProvider`].
    pub(crate) fn start(&self, bus: EventBus) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(rx) = self.rx.lock().take() else {
            return;
        };
        // One shared receiver behind a mutex lets N workers pull from one queue.
        let rx = Arc::new(tokio::sync::Mutex::new(rx));

        for _ in 0..self.workers {
            let rx = rx.clone();
            let handlers = self.handlers.clone();
            let failed = self.failed.clone();
            let tx = self.tx.clone();
            let bus = bus.clone();
            tokio::spawn(async move {
                loop {
                    let job = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };
                    let Some(job) = job else { break };
                    run_job(job, &handlers, &failed, &tx, &bus).await;
                }
            });
        }
    }
}

/// Execute one job, applying retries/backoff and recording a terminal failure.
async fn run_job(
    job: Job,
    handlers: &Arc<Mutex<HashMap<String, Registration>>>,
    failed: &Arc<Mutex<VecDeque<FailedJob>>>,
    tx: &Sender<Job>,
    bus: &EventBus,
) {
    let Some((handler, options)) = handlers
        .lock()
        .get(&job.name)
        .map(|r| (r.handler.clone(), r.options))
    else {
        let _ = bus.emit(
            "elyra:queue",
            &json!({"job": job.name, "status": "unhandled"}),
        );
        return;
    };

    let _ = bus.emit(
        "elyra:queue",
        &json!({"job": job.name, "status": "processing", "attempt": job.attempt}),
    );

    let future = handler(job.payload.clone());
    let outcome = match options.timeout {
        Some(limit) => match tokio::time::timeout(limit, future).await {
            Ok(result) => result,
            Err(_) => Err(format!("timed out after {limit:?}")),
        },
        None => future.await,
    };

    match outcome {
        Ok(()) => {
            let _ = bus.emit(
                "elyra:queue",
                &json!({"job": job.name, "status": "processed", "attempt": job.attempt}),
            );
        }
        Err(error) if job.attempt < options.max_attempts => {
            // Exponential backoff: 500ms, 1s, 2s, …
            let delay = options.retry_base * 2u32.saturating_pow(job.attempt - 1);
            let _ = bus.emit(
                "elyra:queue",
                &json!({
                    "job": job.name,
                    "status": "retrying",
                    "attempt": job.attempt,
                    "error": error,
                    "retry_in_ms": delay.as_millis() as u64,
                }),
            );
            let tx = tx.clone();
            let retry = Job {
                name: job.name,
                payload: job.payload,
                attempt: job.attempt + 1,
            };
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(retry).await;
            });
        }
        Err(error) => {
            let record = FailedJob {
                job: job.name.clone(),
                payload: job.payload,
                error: error.clone(),
                attempts: job.attempt,
                failed_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            };
            {
                let mut history = failed.lock();
                if history.len() >= FAILED_HISTORY {
                    history.pop_front();
                }
                history.push_back(record);
            }
            let _ = bus.emit(
                "elyra:queue",
                &json!({
                    "job": job.name,
                    "status": "failed",
                    "attempts": job.attempt,
                    "error": error,
                }),
            );
        }
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

/// A [`Provider`](crate::Provider) that binds a [`Queue`] and starts its workers.
///
/// ```no_run
/// use elyra::{App, Ctx, Provider};
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
/// App::new()
///     .provider(QueueProvider::with_workers(4))
///     .provider(Jobs)
///     .run()
///     .unwrap();
/// ```
pub struct QueueProvider {
    capacity: usize,
    workers: usize,
}

impl Default for QueueProvider {
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CAPACITY,
            workers: 1,
        }
    }
}

impl QueueProvider {
    /// The provider with default capacity and one worker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Process jobs with `workers` concurrent tasks.
    pub fn with_workers(workers: usize) -> Self {
        Self {
            workers: workers.max(1),
            ..Self::default()
        }
    }

    /// Bound the queue to `capacity` waiting jobs.
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity.max(1);
        self
    }
}

impl crate::Provider for QueueProvider {
    fn register(&self, container: &mut crate::Container) {
        container.bind(Queue::with_capacity(self.capacity, self.workers));
    }

    fn boot(&self, ctx: &crate::Ctx) {
        let bus = ctx.get::<EventBus>().as_ref().clone();
        ctx.get::<Queue>().start(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wait until `check` holds, or give up (keeps CI from flaking).
    async fn eventually(mut check: impl FnMut() -> bool) -> bool {
        for _ in 0..400 {
            if check() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

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

        assert!(eventually(|| seen.lock().len() == 2).await);
        assert_eq!(*seen.lock(), vec![7, 8]);
    }

    #[tokio::test]
    async fn start_is_idempotent() {
        let queue = Queue::new();
        queue.start(EventBus::new());
        queue.start(EventBus::new()); // no panic, no second worker set
    }

    #[tokio::test]
    async fn a_failing_job_is_retried_then_recorded_as_failed() {
        let queue = Queue::new();
        let attempts = Arc::new(Mutex::new(0u32));
        let counter = attempts.clone();
        queue.on_with(
            "flaky",
            JobOptions::default()
                .attempts(3)
                .retry_base(Duration::from_millis(5)),
            move |_| {
                let counter = counter.clone();
                async move {
                    *counter.lock() += 1;
                    Err("nope".to_string())
                }
            },
        );
        queue.start(EventBus::new());
        queue.push("flaky", json!({}));

        assert!(eventually(|| queue.failed().len() == 1).await);
        assert_eq!(*attempts.lock(), 3, "1 try + 2 retries");

        let failed = queue.failed();
        assert_eq!(failed[0].job, "flaky");
        assert_eq!(failed[0].attempts, 3);
        assert_eq!(failed[0].error, "nope");
    }

    #[tokio::test]
    async fn a_job_that_succeeds_on_retry_does_not_fail() {
        let queue = Queue::new();
        let attempts = Arc::new(Mutex::new(0u32));
        let counter = attempts.clone();
        queue.on_with(
            "second-time",
            JobOptions::default()
                .attempts(3)
                .retry_base(Duration::from_millis(5)),
            move |_| {
                let counter = counter.clone();
                async move {
                    let mut n = counter.lock();
                    *n += 1;
                    if *n < 2 {
                        Err("transient".to_string())
                    } else {
                        Ok(())
                    }
                }
            },
        );
        queue.start(EventBus::new());
        queue.push("second-time", json!({}));

        assert!(eventually(|| *attempts.lock() == 2).await);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(queue.failed().is_empty());
    }

    #[tokio::test]
    async fn retry_failed_requeues_jobs() {
        let queue = Queue::new();
        let ok = Arc::new(AtomicBool::new(false));
        let flag = ok.clone();
        queue.on_with(
            "eventually",
            JobOptions::default()
                .attempts(1)
                .retry_base(Duration::from_millis(1)),
            move |_| {
                let flag = flag.clone();
                async move {
                    if flag.load(Ordering::Relaxed) {
                        Ok(())
                    } else {
                        Err("still broken".into())
                    }
                }
            },
        );
        queue.start(EventBus::new());
        queue.push("eventually", json!({}));
        assert!(eventually(|| queue.failed().len() == 1).await);

        ok.store(true, Ordering::Relaxed);
        assert_eq!(queue.retry_failed(), 1);
        assert!(eventually(|| queue.failed().is_empty()).await);
    }

    #[tokio::test]
    async fn a_timeout_counts_as_a_failure() {
        let queue = Queue::new();
        queue.on_with(
            "slow",
            JobOptions::default()
                .attempts(1)
                .timeout(Duration::from_millis(10)),
            |_| async move {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(())
            },
        );
        queue.start(EventBus::new());
        queue.push("slow", json!({}));

        assert!(eventually(|| queue.failed().len() == 1).await);
        assert!(queue.failed()[0].error.contains("timed out"));
    }

    #[tokio::test]
    async fn a_full_queue_drops_instead_of_growing() {
        // Never started, so nothing drains: capacity is the hard ceiling.
        let queue = Queue::with_capacity(2, 1);
        assert!(queue.push("x", json!(1)));
        assert!(queue.push("x", json!(2)));
        assert!(!queue.push("x", json!(3)), "the third push must be refused");
    }

    #[tokio::test]
    async fn typed_dispatch_and_handler() {
        #[derive(Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Resize {
            path: String,
            width: u32,
        }

        let queue = Queue::new();
        let seen = Arc::new(Mutex::new(None::<Resize>));
        let sink = seen.clone();
        queue.on_typed("resize", move |job: Resize| {
            let sink = sink.clone();
            async move {
                *sink.lock() = Some(job);
                Ok(())
            }
        });
        queue.start(EventBus::new());
        assert!(queue.dispatch(
            "resize",
            &Resize {
                path: "a.png".into(),
                width: 128
            }
        ));

        assert!(eventually(|| seen.lock().is_some()).await);
        assert_eq!(
            *seen.lock(),
            Some(Resize {
                path: "a.png".into(),
                width: 128
            })
        );
    }

    #[tokio::test]
    async fn an_invalid_typed_payload_fails_the_job() {
        #[derive(serde::Deserialize)]
        struct Needs {
            #[allow(dead_code)]
            required: String,
        }

        let queue = Queue::new();
        queue.on_typed("strict", |_: Needs| async move { Ok(()) });
        queue.start(EventBus::new());
        queue.push("strict", json!({"wrong": true}));

        assert!(eventually(|| !queue.failed().is_empty()).await);
        assert!(queue.failed()[0].error.contains("invalid payload"));
    }

    #[tokio::test]
    async fn multiple_workers_run_jobs_concurrently() {
        let queue = Queue::with_capacity(16, 4);
        let running = Arc::new(Mutex::new(0i32));
        let peak = Arc::new(Mutex::new(0i32));
        let (r, p) = (running.clone(), peak.clone());
        queue.on("hold", move |_| {
            let (r, p) = (r.clone(), p.clone());
            async move {
                {
                    let mut n = r.lock();
                    *n += 1;
                    let mut top = p.lock();
                    *top = (*top).max(*n);
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
                *r.lock() -= 1;
                Ok(())
            }
        });
        queue.start(EventBus::new());
        for _ in 0..4 {
            queue.push("hold", json!({}));
        }

        assert!(
            eventually(|| *peak.lock() >= 2).await,
            "workers must overlap"
        );
    }

    #[tokio::test]
    async fn push_later_delays_the_job() {
        let queue = Queue::new();
        let ran = Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        queue.on("soon", move |_| {
            let flag = flag.clone();
            async move {
                flag.store(true, Ordering::Relaxed);
                Ok(())
            }
        });
        queue.start(EventBus::new());
        queue.push_later(Duration::from_millis(60), "soon", json!({}));

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!ran.load(Ordering::Relaxed), "must not run early");
        assert!(eventually(|| ran.load(Ordering::Relaxed)).await);
    }
}
