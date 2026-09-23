# Scheduler

Recurring background jobs — the desktop counterpart to Laravel's `Schedule`.
Register async tasks and they run on background tasks for the life of the app.

Add the provider and register jobs in a provider's `boot`:

```rust
use elyra::{App, Ctx, Provider};
use elyra::scheduler::{Scheduler, SchedulerProvider};
use std::time::Duration;

struct Jobs;
impl Provider for Jobs {
    fn boot(&self, ctx: &Ctx) {
        let sched = ctx.get::<Scheduler>();

        sched.every(Duration::from_secs(30), "poll", || async {
            // … refresh something …
        });
        sched.hourly("digest", || async { /* … */ });
        sched.daily("cleanup", || async { /* … */ });
    }
}

App::new().provider(SchedulerProvider).provider(Jobs).run()?;
```

## Intervals

| Method | Runs |
| --- | --- |
| `every(Duration, name, task)` | every interval |
| `every_minutes(n, name, task)` | every `n` minutes |
| `hourly(name, task)` | every hour |
| `daily(name, task)` | every 24 hours |

Intervals are measured **from app start**: `daily()` means "every 24 hours",
not "at midnight". For a time of day, use a clock job.

## Clock times

```rust
use elyra::scheduler::Weekday;

sched.daily_at("09:00", "report", || async { /* … */ });
sched.weekdays_at("08:30", "standup", || async { /* … */ });
sched.weekly_on(Weekday::Monday, "07:00", "digest", || async { /* … */ });
sched.monthly_on(1, "00:00", "invoices", || async { /* … */ });
sched.hourly_at(15, "sync", || async { /* … */ });            // every hour at :15
sched.cron("*/15 9-17 * * mon-fri", "poll", || async { /* … */ });
```

`cron` takes a standard five-field expression — `minute hour day month
weekday` — with `*`, values, ranges (`9-17`), steps (`*/15`, `5/20`), lists
(`8,12,18`) and three-letter names (`mon-fri`, `jan,jul`). Weekday `0` and `7`
are both Sunday. When both day fields are restricted, a day matches if **either**
does: `0 9 15 * mon` runs on the 15th *and* every Monday. An invalid literal
panics at registration, saying what is wrong; for expressions from user input,
parse with `Cron::parse` (which returns a `Result`) and register with `at`.

Times are in the **system time zone**, re-read on every run, so a user who
travels picks up the new zone.

## Semantics

- **Sleep and wake.** A laptop that sleeps through a job's time runs it once,
  promptly after waking — clock jobs re-check the wall clock at least every 30
  seconds instead of trusting one long timer, whose monotonic clock stops while
  the machine is suspended. Occurrences missed while asleep are not replayed.
- **Daylight saving time.** A time skipped by a spring-forward runs just after
  the gap (`02:30` on that night runs at `03:30`); a time repeated by a fall-back
  runs once.
- **No overlap.** A job's next run is computed only after the current one
  finishes, so a slow run is never doubled up — Laravel's `withoutOverlapping()`
  is the default. A run that overshoots the next occurrence skips it.
- Jobs run in their own background task; a slow job doesn't delay the others.
- Registration works **before or after** the scheduler starts, so provider order
  doesn't matter.
- In-process only: jobs stop when the app exits. For work that must survive a
  restart, have the scheduled task push a job onto a
  [durable queue](queue.md#durable-queues).

## Related

- [Queue](queue.md) — one-off background jobs. · [Container & providers](container-and-providers.md)
