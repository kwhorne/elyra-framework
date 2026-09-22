//! An ergonomic background **queue** facade — the desktop-side counterpart to
//! Laravel's `Queue::` / Askr's supervised queue workers. Same surface (`push`
//! a named job, register a handler), but scoped to a single process.
//!
//! **In memory by default; durable on request.** A plain queue loses its jobs on
//! exit. [`QueueProvider::durable`] backs it with two tables in the app's
//! database (Laravel's `jobs` and `failed_jobs`, feature `database`): pending,
//! delayed and retrying jobs survive a restart and run again on the next launch,
//! and failed jobs are kept until retried or cleared. Delivery is
//! *at least once* — a job that was running when the app died runs again — so
//! handlers should be idempotent. It is not cross-process: one app instance per
//! journal. A worker fleet is Askr's domain on the server; here it's for
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

#[cfg(feature = "database")]
mod journal;

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
type JournalFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Where a durable queue keeps its jobs. One implementation (the database
/// journal); a trait so the queue's own logic stays free of `cfg(feature)`.
trait Journal: Send + Sync {
    /// Record a new job; its id travels with the job from then on.
    fn insert<'a>(
        &'a self,
        job: &'a str,
        payload: &'a Value,
        available_at_ms: u64,
    ) -> JournalFuture<'a, i64>;
    /// A retry: bump the attempt and the time it becomes available again.
    fn reschedule(&self, id: i64, attempt: u32, available_at_ms: u64) -> JournalFuture<'_, ()>;
    /// The job succeeded; forget it.
    fn complete(&self, id: i64) -> JournalFuture<'_, ()>;
    /// Move a job into the failed table (in one transaction); returns the failed
    /// row's id. `id: None` records a failure for a job that was never journaled.
    fn fail<'a>(&'a self, id: Option<i64>, failed: &'a FailedJob) -> JournalFuture<'a, i64>;
    /// Delete failed rows (`None` = all of them).
    fn forget_failed(&self, ids: Option<Vec<i64>>) -> JournalFuture<'_, ()>;
    /// What a previous run left behind — taken once, before this process wrote.
    fn recovered(&self) -> JournalFuture<'_, Recovered>;
}

/// A job read back from the journal.
#[cfg_attr(not(feature = "database"), allow(dead_code))]
struct StoredJob {
    id: i64,
    name: String,
    payload: Value,
    attempt: u32,
    available_at_ms: u64,
}

/// The startup snapshot a journal hands to the queue.
#[cfg_attr(not(feature = "database"), allow(dead_code))]
#[derive(Default)]
struct Recovered {
    pending: Vec<StoredJob>,
    failed: Vec<(i64, FailedJob)>,
}

/// Milliseconds since the Unix epoch (journal timestamps).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Failed jobs in memory, each with its journal row id when durable.
type FailedHistory = Arc<Mutex<VecDeque<(Option<i64>, FailedJob)>>>;

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
    /// The journal row, when the queue is durable.
    id: Option<i64>,
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
    failed: FailedHistory,
    workers: usize,
    started: AtomicBool,
    /// Set (before `start`) for a durable queue.
    journal: Mutex<Option<Arc<dyn Journal>>>,
    recovered: AtomicBool,
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
            journal: Mutex::new(None),
            recovered: AtomicBool::new(false),
        }
    }

    /// Back this queue with the app database (see [`QueueProvider::durable`]).
    /// Must happen before [`start`](Queue::start).
    #[cfg(feature = "database")]
    pub(crate) fn use_database(&self, db: Arc<elyra_db::Database>) {
        *self.journal.lock() = Some(Arc::new(journal::DbJournal::new(db)));
    }

    fn journal(&self) -> Option<Arc<dyn Journal>> {
        self.journal.lock().clone()
    }

    /// Whether jobs survive a restart.
    pub fn is_durable(&self) -> bool {
        self.journal.lock().is_some()
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
    ///
    /// On a durable queue the slot is reserved immediately — so backpressure
    /// works the same — and the job is written to the journal *before* it can
    /// run, a moment after this returns. Use [`push_confirmed`](Queue::push_confirmed)
    /// to wait until the write has committed.
    pub fn push(&self, job: impl Into<String>, payload: impl Into<Value>) -> bool {
        let job = Job {
            id: None,
            name: job.into(),
            payload: payload.into(),
            attempt: 1,
        };
        let Some(journal) = self.journal() else {
            return self.enqueue(job);
        };
        let Some(permit) = self.reserve(&job.name) else {
            return false;
        };
        tokio::spawn(async move {
            let job = journaled(journal.as_ref(), job, now_ms()).await;
            permit.send(job);
        });
        true
    }

    /// Enqueue a job and wait until it is durable — the journal row has been
    /// committed — before returning. On an in-memory queue it's just [`push`](Queue::push).
    /// Errs when the queue is full or the write failed (then nothing was enqueued).
    pub async fn push_confirmed(
        &self,
        job: impl Into<String>,
        payload: impl Into<Value>,
    ) -> crate::Result<()> {
        let name = job.into();
        let payload = payload.into();
        let Some(journal) = self.journal() else {
            return if self.push(name, payload) {
                Ok(())
            } else {
                Err(crate::Error::command("queue is full; job was not enqueued"))
            };
        };
        let permit = self
            .reserve(&name)
            .ok_or_else(|| crate::Error::command("queue is full; job was not enqueued"))?;
        let id = journal
            .insert(&name, &payload, now_ms())
            .await
            .map_err(crate::Error::Io)?;
        permit.send(Job {
            id: Some(id),
            name,
            payload,
            attempt: 1,
        });
        Ok(())
    }

    /// Reserve a slot in the channel now, so a durable push reports backpressure
    /// synchronously even though its journal write happens later.
    fn reserve(&self, name: &str) -> Option<mpsc::OwnedPermit<Job>> {
        match self.tx.clone().try_reserve_owned() {
            Ok(permit) => Some(permit),
            Err(mpsc::error::TrySendError::Full(_)) => {
                crate::warn!(target: "elyra::queue", "queue is full; dropping job `{name}`");
                None
            }
            Err(mpsc::error::TrySendError::Closed(_)) => None,
        }
    }

    /// Enqueue a **typed** payload (serialized with serde).
    pub fn dispatch<T: Serialize>(&self, job: impl Into<String>, payload: &T) -> bool {
        match serde_json::to_value(payload) {
            Ok(value) => self.push(job, value),
            Err(_) => false,
        }
    }

    /// Enqueue a job to run after `delay`. On a durable queue the delay survives
    /// a restart: the job runs once the remaining time has passed.
    pub fn push_later(&self, delay: Duration, job: impl Into<String>, payload: impl Into<Value>) {
        let job = Job {
            id: None,
            name: job.into(),
            payload: payload.into(),
            attempt: 1,
        };
        let journal = self.journal();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let job = match journal {
                Some(journal) => {
                    journaled(journal.as_ref(), job, now_ms() + delay.as_millis() as u64).await
                }
                None => job,
            };
            tokio::time::sleep(delay).await;
            let _ = tx.send(job).await;
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

    /// Jobs that exhausted their attempts (most recent last). A durable queue
    /// also loads the last 100 from the previous run; the table keeps them all.
    pub fn failed(&self) -> Vec<FailedJob> {
        self.failed.lock().iter().map(|(_, f)| f.clone()).collect()
    }

    /// Forget the failed-job history (and, on a durable queue, the table).
    pub fn clear_failed(&self) {
        self.failed.lock().clear();
        if let Some(journal) = self.journal() {
            tokio::spawn(async move {
                if let Err(e) = journal.forget_failed(None).await {
                    crate::error!(target: "elyra::queue", "{e}");
                }
            });
        }
    }

    /// Re-enqueue every failed job (a local `queue:retry`).
    pub fn retry_failed(&self) -> usize {
        let jobs: Vec<(Option<i64>, FailedJob)> = self.failed.lock().drain(..).collect();
        let mut requeued = 0;
        let mut forget = Vec::new();
        for (failed_id, failed) in jobs {
            if self.push(failed.job, failed.payload) {
                requeued += 1;
                forget.extend(failed_id);
            }
        }
        if let Some(journal) = self.journal() {
            tokio::spawn(async move {
                if let Err(e) = journal.forget_failed(Some(forget)).await {
                    crate::error!(target: "elyra::queue", "{e}");
                }
            });
        }
        requeued
    }

    /// Deliver what a previous run left in the journal. Called by the app once
    /// **every** provider has booted, so handlers registered by a later provider
    /// are in place before recovered jobs arrive. Idempotent; a no-op for an
    /// in-memory queue.
    pub(crate) fn recover(&self) {
        let Some(journal) = self.journal() else {
            return;
        };
        if self.recovered.swap(true, Ordering::AcqRel) {
            return;
        }
        let tx = self.tx.clone();
        let failed = self.failed.clone();
        tokio::spawn(async move {
            let recovered = match journal.recovered().await {
                Ok(recovered) => recovered,
                Err(e) => {
                    crate::error!(target: "elyra::queue", "recovery failed: {e}");
                    return;
                }
            };
            {
                // Earlier failures first, then anything that failed this run.
                let mut history = failed.lock();
                for (id, entry) in recovered.failed.into_iter().rev() {
                    history.push_front((Some(id), entry));
                }
                while history.len() > FAILED_HISTORY {
                    history.pop_front();
                }
            }
            let count = recovered.pending.len();
            for stored in recovered.pending {
                let job = Job {
                    id: Some(stored.id),
                    name: stored.name,
                    payload: stored.payload,
                    attempt: stored.attempt,
                };
                let wait = stored.available_at_ms.saturating_sub(now_ms());
                let tx = tx.clone();
                if wait == 0 {
                    let _ = tx.send(job).await;
                } else {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(wait)).await;
                        let _ = tx.send(job).await;
                    });
                }
            }
            if count > 0 {
                crate::info!(target: "elyra::queue", "recovered {count} job(s) from the journal");
            }
        });
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

        let journal = self.journal();
        for _ in 0..self.workers {
            let rx = rx.clone();
            let handlers = self.handlers.clone();
            let failed = self.failed.clone();
            let tx = self.tx.clone();
            let bus = bus.clone();
            let journal = journal.clone();
            tokio::spawn(async move {
                loop {
                    let job = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };
                    let Some(job) = job else { break };
                    run_job(job, &handlers, &failed, &tx, &bus, journal.as_deref()).await;
                }
            });
        }
    }
}

/// Write a job to the journal and attach its id. If the write fails the job
/// still runs — it just won't survive a restart — rather than being lost now.
async fn journaled(journal: &dyn Journal, mut job: Job, available_at_ms: u64) -> Job {
    match journal
        .insert(&job.name, &job.payload, available_at_ms)
        .await
    {
        Ok(id) => job.id = Some(id),
        Err(e) => crate::error!(
            target: "elyra::queue",
            "could not journal `{}` ({e}); it will run but not survive a restart",
            job.name
        ),
    }
    job
}

/// Record a terminal failure: in memory, in the journal, and on `elyra:queue`.
async fn record_failure(
    job: &Job,
    error: String,
    failed: &FailedHistory,
    bus: &EventBus,
    journal: Option<&dyn Journal>,
) {
    let record = FailedJob {
        job: job.name.clone(),
        payload: job.payload.clone(),
        error: error.clone(),
        attempts: job.attempt,
        failed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    let failed_id = match journal {
        Some(journal) => match journal.fail(job.id, &record).await {
            Ok(id) => Some(id),
            Err(e) => {
                crate::error!(target: "elyra::queue", "{e}");
                None
            }
        },
        None => None,
    };
    {
        let mut history = failed.lock();
        if history.len() >= FAILED_HISTORY {
            history.pop_front();
        }
        history.push_back((failed_id, record));
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

/// Execute one job, applying retries/backoff and recording a terminal failure.
async fn run_job(
    job: Job,
    handlers: &Arc<Mutex<HashMap<String, Registration>>>,
    failed: &FailedHistory,
    tx: &Sender<Job>,
    bus: &EventBus,
    journal: Option<&dyn Journal>,
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
        // A journaled job is data the app promised to keep: fail it (visible,
        // retryable) rather than dropping it. Recovery runs after every provider
        // has booted, so a missing handler here is genuinely missing.
        if job.id.is_some() {
            let error = format!("no handler registered for `{}`", job.name);
            record_failure(&job, error, failed, bus, journal).await;
        }
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
            if let (Some(journal), Some(id)) = (journal, job.id) {
                if let Err(e) = journal.complete(id).await {
                    crate::error!(target: "elyra::queue", "{e}");
                }
            }
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
            // Persist the retry first, so a restart during the backoff resumes
            // at the right attempt and time instead of starting over.
            if let (Some(journal), Some(id)) = (journal, job.id) {
                let at = now_ms() + delay.as_millis() as u64;
                if let Err(e) = journal.reschedule(id, job.attempt + 1, at).await {
                    crate::error!(target: "elyra::queue", "{e}");
                }
            }
            let tx = tx.clone();
            let retry = Job {
                id: job.id,
                name: job.name,
                payload: job.payload,
                attempt: job.attempt + 1,
            };
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(retry).await;
            });
        }
        Err(error) => record_failure(&job, error, failed, bus, journal).await,
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
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    durable: bool,
}

impl Default for QueueProvider {
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CAPACITY,
            workers: 1,
            durable: false,
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

    /// Keep jobs in the app's database so they survive a restart — Laravel's
    /// `database` queue driver. Needs a bound [`Database`](elyra_db::Database)
    /// (`App::database(..)`); the `elyra_jobs` / `elyra_failed_jobs` tables are
    /// created on first use.
    ///
    /// ```no_run
    /// # use elyra::{App, queue::QueueProvider};
    /// App::new()
    ///     .database("sqlite://app.db?mode=rwc")
    ///     .provider(QueueProvider::with_workers(2).durable());
    /// ```
    #[cfg(feature = "database")]
    pub fn durable(mut self) -> Self {
        self.durable = true;
        self
    }
}

impl crate::Provider for QueueProvider {
    fn register(&self, container: &mut crate::Container) {
        container.bind(Queue::with_capacity(self.capacity, self.workers));
    }

    fn boot(&self, ctx: &crate::Ctx) {
        let queue = ctx.get::<Queue>();
        #[cfg(feature = "database")]
        if self.durable {
            let db = ctx.try_get::<elyra_db::Database>().expect(
                "QueueProvider::durable() needs a Database: add App::database(..) or bind one",
            );
            queue.use_database(db);
        }
        let bus = ctx.get::<EventBus>().as_ref().clone();
        queue.start(bus);
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
