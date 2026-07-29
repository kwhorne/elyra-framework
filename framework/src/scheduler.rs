//! A background task **scheduler** — the desktop counterpart to Laravel's
//! `Schedule`. Register recurring async jobs (`every` / `every_minutes` /
//! `hourly` / `daily`) and they run on background tasks.
//!
//! Intervals are measured **from when the app starts**, not wall-clock times
//! (`daily()` means "every 24 hours", not "at midnight") — a good fit for an
//! always-running desktop process without pulling in a calendar/timezone
//! dependency.
//!
//! Add [`SchedulerProvider`] and register jobs in a provider's `boot`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
type Task = Arc<dyn Fn() -> BoxFuture + Send + Sync>;

struct Job {
    /// Carried for future logging/inspection.
    #[allow(dead_code)]
    name: String,
    interval: Duration,
    task: Task,
}

struct State {
    jobs: Vec<Job>,
    started: bool,
}

/// Registers and runs recurring background jobs.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Mutex<State>>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                jobs: Vec::new(),
                started: false,
            })),
        }
    }

    /// Run `task` every `interval`. Registration works before or after the
    /// scheduler starts (late registrations spawn immediately).
    pub fn every<F, Fut>(&self, interval: Duration, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let task = Arc::new(task);
        let job = Job {
            name: name.into(),
            interval,
            task: Arc::new(move || {
                let task = task.clone();
                Box::pin(async move { task().await })
            }),
        };
        let mut state = self.inner.lock();
        if state.started {
            spawn_job(job);
        } else {
            state.jobs.push(job);
        }
    }

    /// Run every `minutes` minutes.
    pub fn every_minutes<F, Fut>(&self, minutes: u64, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.every(Duration::from_secs(minutes * 60), name, task);
    }

    /// Run once an hour.
    pub fn hourly<F, Fut>(&self, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.every(Duration::from_secs(3600), name, task);
    }

    /// Run once every 24 hours (from start).
    pub fn daily<F, Fut>(&self, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.every(Duration::from_secs(86_400), name, task);
    }

    /// Start the scheduler: spawn a loop per registered job. Idempotent. Called
    /// by [`SchedulerProvider`].
    pub(crate) fn start(&self) {
        let mut state = self.inner.lock();
        if state.started {
            return;
        }
        state.started = true;
        for job in state.jobs.drain(..) {
            spawn_job(job);
        }
    }
}

/// Spawn a job's loop: wait `interval`, run, repeat. No-op outside a runtime.
fn spawn_job(job: Job) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(job.interval).await;
            (job.task)().await;
        }
    });
}

/// A [`Provider`](crate::Provider) that binds a [`Scheduler`] and starts it.
///
/// ```no_run
/// use elyra::{App, Ctx, Provider};
/// use elyra::scheduler::{Scheduler, SchedulerProvider};
/// use std::time::Duration;
///
/// struct Jobs;
/// impl Provider for Jobs {
///     fn boot(&self, ctx: &Ctx) {
///         ctx.get::<Scheduler>().every(Duration::from_secs(300), "cleanup", || async {
///             // … periodic work …
///         });
///     }
/// }
///
/// App::new().provider(SchedulerProvider).provider(Jobs).run().unwrap();
/// ```
pub struct SchedulerProvider;

impl crate::Provider for SchedulerProvider {
    fn register(&self, container: &mut crate::Container) {
        container.bind(Scheduler::new());
    }

    fn boot(&self, ctx: &crate::Ctx) {
        ctx.get::<Scheduler>().start();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn runs_registered_jobs_on_interval() {
        let scheduler = Scheduler::new();
        let hits = Arc::new(AtomicU32::new(0));
        let h = hits.clone();
        scheduler.every(Duration::from_millis(10), "tick", move || {
            let h = h.clone();
            async move {
                h.fetch_add(1, Ordering::Relaxed);
            }
        });
        scheduler.start();
        // ~2s budget for the job to fire at least twice.
        for _ in 0..200 {
            if hits.load(Ordering::Relaxed) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(hits.load(Ordering::Relaxed) >= 2);
    }

    #[tokio::test]
    async fn start_is_idempotent() {
        let scheduler = Scheduler::new();
        scheduler.start();
        scheduler.start();
    }
}
