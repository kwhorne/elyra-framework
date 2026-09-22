# Queue

An ergonomic background **queue** facade — the desktop-side counterpart to
[Askr](https://github.com/kwhorne/askr)/Laravel's `Queue::`. One ecosystem: you
`push` a named job and register a handler, the same way in both worlds. Here
jobs run on a background task, in order, in the same process.

> **Scope.** In memory by default — jobs are lost on exit — or
> [durable](#durable-queues) with one call, backed by the app's database. Either
> way it's single-process: there's no separate worker fleet — that's Askr's job
> on the server. On the desktop this is for getting work off the UI thread
> (exports, uploads, cleanup) with familiar ergonomics.

Add the provider, and register handlers in a provider's `boot`:

```rust
use elyra::{App, Ctx, Provider};
use elyra::queue::{Queue, QueueProvider};

struct Jobs;
impl Provider for Jobs {
    fn boot(&self, ctx: &Ctx) {
        ctx.get::<Queue>().on("resize_image", |payload| async move {
            let path = payload["path"].as_str().unwrap_or_default().to_string();
            // … do the slow work …
            Ok(())
        });
    }
}

App::new().provider(QueueProvider).provider(Jobs).run()?;
```

## Pushing jobs

From a command (or anywhere with the container):

```rust
#[command]
async fn resize(ctx: Ctx, path: String) {
    ctx.get::<Queue>().push("resize_image", serde_json::json!({ "path": path }));
}
```

From the frontend:

```ts
import { queue, onQueue } from "@elyra/runtime";

onQueue((e) => {
  // { job, status: "processing" | "processed" | "failed" | "unhandled", error? }
  if (e.status === "failed") console.error(e.job, e.error);
});

await queue.push("resize_image", { path: "/tmp/in.png" });
```

Handlers are **Rust-side** (like Laravel jobs run on the server). The frontend
enqueues and observes status on `elyra:queue`; it doesn't run job code.

## Retries, timeouts and failed jobs

```rust
use elyra::queue::JobOptions;
use std::time::Duration;

queue.on_with(
    "upload",
    JobOptions::default()
        .attempts(5)                              // 1 try + 4 retries
        .retry_base(Duration::from_secs(1))       // 1s, 2s, 4s, 8s
        .timeout(Duration::from_secs(30)),        // per attempt
    |payload| async move { upload(payload).await.map_err(|e| e.to_string()) },
);
```

A job that exhausts its attempts lands in the failed-job list — the local
stand-in for Laravel's `failed_jobs` table:

```rust
for failed in queue.failed() {
    eprintln!("{} failed after {} attempts: {}", failed.job, failed.attempts, failed.error);
}
queue.retry_failed();   // re-enqueue everything
queue.clear_failed();
```

## Typed jobs

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct Resize { path: String, width: u32 }

queue.on_typed("resize", |job: Resize| async move {
    resize(&job.path, job.width).await.map_err(|e| e.to_string())
});

queue.dispatch("resize", &Resize { path: "a.png".into(), width: 128 });
```

A payload that doesn't deserialize fails the job with `invalid payload: …`
(and is retried like any other error).

## Concurrency, capacity and delays

```rust
App::new().provider(QueueProvider::with_workers(4).with_capacity(2048))
```

```rust
queue.push_later(Duration::from_secs(60), "cleanup", json!({}));
```

## Behavior

- The queue is **bounded** (default 1024 waiting jobs). `push` returns `false`
  when it's full — backpressure instead of growing until the process dies. The
  frontend's `queue.push` rejects in that case.
- With one worker jobs run in order; `with_workers(n)` processes up to `n` at once.
- Status on `elyra:queue`: `processing` (with `attempt`), `processed`, `retrying`
  (with `error` + `retry_in_ms`), `failed` (with `attempts`), `unhandled`.
- In memory unless [durable](#durable-queues): an in-memory queue loses its jobs
  on exit.

## Durable queues

`durable()` keeps every job in the app's database until it succeeds or finally
fails — Laravel's `database` queue driver. Quit the app with 200 uploads
waiting, and they run on the next launch.

```rust
App::new()
    .database("sqlite://app.db?mode=rwc")                 // or bind a Database
    .provider(QueueProvider::with_workers(2).durable())
    .provider(Jobs);                                      // registers handlers in boot
```

Two tables, created on first use — no migration to run:

| Table | Holds |
|---|---|
| `elyra_jobs` | every pending, delayed or retrying job |
| `elyra_failed_jobs` | jobs that exhausted their attempts, until `retry_failed` / `clear_failed` |

What survives a restart:

- **Pending jobs**, including the waiting backlog: a job enters the worker queue
  only *after* it's on disk, so nothing is ever held only in memory.
- **Delays and retries**: `push_later` and a retry's backoff resume with the
  *remaining* time, at the right attempt number.
- **Failed jobs**: `queue.failed()` lists the last 100 from earlier runs (the
  table keeps them all); `retry_failed()` re-enqueues them and removes the rows.

Semantics worth knowing:

- **At least once.** A job that was running when the app died runs again on the
  next launch, so make handlers idempotent (an upload keyed by content hash, an
  export that overwrites its file).
- **`push` vs `push_confirmed`.** `push` reserves a slot immediately — so it
  still returns `false` on a full queue — and writes the row a moment later.
  `push_confirmed(..).await` returns once the row is committed; use it when the
  caller must know the job is safe (e.g. before telling the user "queued").
- **Recovered jobs are delivered after every provider has booted**, so a handler
  registered by a provider added after `QueueProvider` still receives them.
  A recovered job with no handler at all moves to the failed table
  (`no handler registered for …`) instead of vanishing.
- **One app instance per database.** The journal isn't a multi-process lock;
  pair it with [single-instance](single-instance.md) if a second launch could
  open the same file.

## Related

- [Events](events.md) — the `elyra:queue` channel. · [Cache](cache.md) · [Storage](storage.md)
- [Sidecar](sidecar.md) — for long-running external processes.
