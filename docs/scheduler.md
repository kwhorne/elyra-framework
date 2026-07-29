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

## API

| Method | Runs |
| --- | --- |
| `every(Duration, name, task)` | every interval |
| `every_minutes(n, name, task)` | every `n` minutes |
| `hourly(name, task)` | every hour |
| `daily(name, task)` | every 24 hours |

Tasks are async closures (`Fn() -> impl Future<Output = ()>`).

## Semantics

- Intervals are measured **from app start**, not wall-clock times — `daily()`
  means "every 24 hours", not "at midnight". (This keeps the scheduler
  dependency-free; wall-clock scheduling would need a calendar/timezone crate.)
- Jobs run in their own background task; a slow job doesn't delay the others.
- Registration works **before or after** the scheduler starts, so provider order
  doesn't matter.
- In-process only: jobs stop when the app exits (durable, cross-process
  scheduling is a server concern — see [Askr](https://github.com/kwhorne/askr)).

## Related

- [Queue](queue.md) — one-off background jobs. · [Container & providers](container-and-providers.md)
