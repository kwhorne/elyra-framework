//! A background task **scheduler** — the desktop counterpart to Laravel's
//! `Schedule`. Two kinds of job:
//!
//! * **Intervals** — `every` / `every_minutes` / `hourly` / `daily`, measured
//!   from when the app starts (`daily()` is "every 24 hours", not "at midnight").
//! * **Clock times** — `daily_at("09:00")`, `weekdays_at`, `weekly_on`,
//!   `monthly_on`, `hourly_at`, or any five-field [`cron`](Scheduler::cron)
//!   expression, in the system time zone with DST handled.
//!
//! A desktop scheduler has a problem a server's doesn't: **the machine sleeps**.
//! A timer set at 08:00 for 09:00 counts only awake time, so after a lid closed
//! from 07:30 to 10:00 it would fire around 11:30. Clock jobs therefore wait in
//! chunks of at most 30 seconds against the wall clock: after a wake the job
//! runs once, promptly, and the occurrences missed while asleep are not replayed.
//!
//! A job's runs **never overlap** — the next occurrence is computed only once the
//! current run has finished, so Laravel's `withoutOverlapping()` is the default.
//!
//! Add [`SchedulerProvider`] and register jobs in a provider's `boot`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

mod cron;
pub use cron::{Cron, CronError};

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
type Task = Arc<dyn Fn() -> BoxFuture + Send + Sync>;

/// The longest a clock job sleeps before re-reading the wall clock, so a
/// suspended machine is noticed within this much *awake* time.
const WALL_CLOCK_CHECK: Duration = Duration::from_secs(30);

/// When a job runs.
enum Trigger {
    /// Every interval, measured on the monotonic clock from start.
    Every(Duration),
    /// At the times a cron expression matches, in the system time zone.
    Clock(Cron),
}

struct Job {
    name: String,
    trigger: Trigger,
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

    fn add<F, Fut>(&self, trigger: Trigger, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let task = Arc::new(task);
        let job = Job {
            name: name.into(),
            trigger,
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

    /// Run `task` every `interval`. Registration works before or after the
    /// scheduler starts (late registrations spawn immediately).
    pub fn every<F, Fut>(&self, interval: Duration, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.add(Trigger::Every(interval), name, task);
    }

    /// Run at the times a five-field cron expression matches, in the system time
    /// zone — Laravel's `->cron('…')`:
    ///
    /// ```ignore
    /// sched.cron("*/15 9-17 * * mon-fri", "sync", || async { /* … */ });
    /// ```
    ///
    /// # Panics
    /// On an invalid expression, naming what is wrong — a literal schedule is
    /// wiring, so it fails at startup. Parse input from elsewhere with
    /// [`Cron::parse`] and pass it to [`at`](Scheduler::at).
    pub fn cron<F, Fut>(&self, expression: &str, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let cron = Cron::parse(expression).unwrap_or_else(|e| panic!("{e}"));
        self.at(cron, name, task);
    }

    /// Run at the times an already-parsed [`Cron`] matches.
    pub fn at<F, Fut>(&self, cron: Cron, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.add(Trigger::Clock(cron), name, task);
    }

    /// Run every day at `time` (`"09:00"`, 24-hour, local time).
    pub fn daily_at<F, Fut>(&self, time: &str, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (h, m) = clock_time(time);
        self.cron(&format!("{m} {h} * * *"), name, task);
    }

    /// Run Monday to Friday at `time`.
    pub fn weekdays_at<F, Fut>(&self, time: &str, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (h, m) = clock_time(time);
        self.cron(&format!("{m} {h} * * 1-5"), name, task);
    }

    /// Run once a week, on `weekday` at `time`.
    pub fn weekly_on<F, Fut>(&self, weekday: Weekday, time: &str, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (h, m) = clock_time(time);
        let day = weekday.to_sunday_zero_offset();
        self.cron(&format!("{m} {h} * * {day}"), name, task);
    }

    /// Run once a month, on `day` (1-31) at `time`. Months without that day are
    /// skipped (`monthly_on(31, ..)` doesn't run in April).
    pub fn monthly_on<F, Fut>(&self, day: u8, time: &str, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (h, m) = clock_time(time);
        self.cron(&format!("{m} {h} {day} * *"), name, task);
    }

    /// Run every hour at `minute` past (0-59) — `hourly_at(15, ..)` is :15.
    pub fn hourly_at<F, Fut>(&self, minute: u8, name: impl Into<String>, task: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.cron(&format!("{minute} * * * *"), name, task);
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

/// The days of the week, for [`Scheduler::weekly_on`].
pub use jiff::civil::Weekday;

/// Parse `"HH:MM"` (24-hour). A malformed literal is wiring, so it panics.
fn clock_time(time: &str) -> (u8, u8) {
    let digits = |s: &str, len: std::ops::RangeInclusive<usize>| {
        len.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
    };
    let parsed = time.split_once(':').and_then(|(h, m)| {
        // The text, not the parsed value: `09:000` must not read as 09:00.
        if !digits(h, 1..=2) || !digits(m, 2..=2) {
            return None;
        }
        let (h, m): (u8, u8) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some((h, m))
    });
    parsed.unwrap_or_else(|| panic!("invalid time `{time}`: expected \"HH:MM\", 24-hour"))
}

/// Spawn a job's loop. No-op outside a runtime.
fn spawn_job(job: Job) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        match &job.trigger {
            Trigger::Every(interval) => loop {
                tokio::time::sleep(*interval).await;
                (job.task)().await;
            },
            Trigger::Clock(cron) => {
                let cron = cron.clone();
                run_on_clock(move |now| cron.next_after(now), &job.name, job.task.clone()).await
            }
        }
    });
}

/// Run `task` at each time `next` yields, waiting against the wall clock.
/// Separate from [`Cron`] so the loop can be tested with near-future times.
async fn run_on_clock(next: impl Fn(&jiff::Zoned) -> Option<jiff::Zoned>, name: &str, task: Task) {
    loop {
        let now = jiff::Zoned::now();
        let Some(due) = next(&now) else {
            crate::warn!(target: "elyra::scheduler", "`{name}` has no future run; stopping it");
            return;
        };
        sleep_until_wall(due.timestamp()).await;
        task().await;
    }
}

/// Sleep until the wall clock reaches `due`, in chunks of at most
/// [`WALL_CLOCK_CHECK`] — the monotonic clock behind a single long sleep stops
/// while the machine is suspended, and would run the job late by that much.
async fn sleep_until_wall(due: jiff::Timestamp) {
    loop {
        let remaining = jiff::Timestamp::now().duration_until(due);
        if remaining.is_zero() || remaining.is_negative() {
            return;
        }
        let remaining = Duration::try_from(remaining).unwrap_or(WALL_CLOCK_CHECK);
        tokio::time::sleep(remaining.min(WALL_CLOCK_CHECK)).await;
    }
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
    async fn a_clock_job_runs_at_the_times_it_is_given_and_never_overlaps() {
        // Near-future "clock" times, so the real loop runs without a real cron wait.
        let runs = Arc::new(AtomicU32::new(0));
        let running = Arc::new(AtomicU32::new(0));
        let peak = Arc::new(AtomicU32::new(0));
        let (r, g, p) = (runs.clone(), running.clone(), peak.clone());
        let task: Task = Arc::new(move || {
            let (r, g, p) = (r.clone(), g.clone(), p.clone());
            Box::pin(async move {
                let now = g.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(now, Ordering::SeqCst);
                // Longer than the gap between due times: a second run would
                // overlap if the loop didn't wait for this one.
                tokio::time::sleep(Duration::from_millis(80)).await;
                g.fetch_sub(1, Ordering::SeqCst);
                r.fetch_add(1, Ordering::SeqCst);
            })
        });
        let handle = tokio::spawn(run_on_clock(
            |now| now.checked_add(jiff::SignedDuration::from_millis(20)).ok(),
            "tick",
            task,
        ));
        for _ in 0..300 {
            if runs.load(Ordering::SeqCst) >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        handle.abort();
        assert!(runs.load(Ordering::SeqCst) >= 3, "it kept running");
        assert_eq!(peak.load(Ordering::SeqCst), 1, "runs never overlapped");
    }

    #[tokio::test]
    async fn a_schedule_with_no_future_run_stops_quietly() {
        let task: Task = Arc::new(|| Box::pin(async {}));
        let finished = tokio::time::timeout(
            Duration::from_secs(1),
            run_on_clock(|_| None, "never", task),
        )
        .await;
        assert!(finished.is_ok(), "the loop must end, not spin");
    }

    #[tokio::test]
    async fn sleep_until_wall_returns_for_past_and_near_times() {
        let past = jiff::Timestamp::now() - jiff::SignedDuration::from_secs(5);
        tokio::time::timeout(Duration::from_millis(100), sleep_until_wall(past))
            .await
            .expect("a due time in the past returns at once");
        let soon = jiff::Timestamp::now() + jiff::SignedDuration::from_millis(50);
        let started = std::time::Instant::now();
        sleep_until_wall(soon).await;
        assert!(started.elapsed() >= Duration::from_millis(40));
    }

    #[test]
    fn clock_times_parse_and_reject_nonsense() {
        assert_eq!(clock_time("09:00"), (9, 0));
        assert_eq!(clock_time("23:59"), (23, 59));
        assert_eq!(clock_time("7:05"), (7, 5));
        for bad in [
            "24:00", "12:60", "noon", "9", "09:000", "9:5", " 9:05", "-1:00",
        ] {
            let err = std::panic::catch_unwind(|| clock_time(bad)).expect_err(bad);
            let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(msg.contains("expected \"HH:MM\""), "{bad}: {msg}");
        }
    }

    #[test]
    fn the_helpers_build_the_expressions_they_describe() {
        // Registered before start, so nothing spawns; inspect what was stored.
        let s = Scheduler::new();
        s.daily_at("09:30", "a", || async {});
        s.weekdays_at("08:00", "b", || async {});
        s.weekly_on(Weekday::Monday, "07:15", "c", || async {});
        s.monthly_on(1, "00:00", "d", || async {});
        s.hourly_at(45, "e", || async {});
        let exprs: Vec<String> = s
            .inner
            .lock()
            .jobs
            .iter()
            .map(|j| match &j.trigger {
                Trigger::Clock(c) => c.to_string(),
                Trigger::Every(d) => format!("every {d:?}"),
            })
            .collect();
        assert_eq!(
            exprs,
            [
                "30 9 * * *",
                "0 8 * * 1-5",
                "15 7 * * 1",
                "0 0 1 * *",
                "45 * * * *"
            ]
        );
    }

    #[test]
    #[should_panic(expected = "invalid cron expression `61 * * * *`")]
    fn an_invalid_cron_literal_fails_at_registration() {
        Scheduler::new().cron("61 * * * *", "bad", || async {});
    }

    #[tokio::test]
    async fn start_is_idempotent() {
        let scheduler = Scheduler::new();
        scheduler.start();
        scheduler.start();
    }
}
