# Queue

An ergonomic background **queue** facade — the desktop-side counterpart to
[Askr](https://github.com/kwhorne/askr)/Laravel's `Queue::`. One ecosystem: you
`push` a named job and register a handler, the same way in both worlds. Here
jobs run on a background task, in order, in the same process.

> **Scope.** Not durable and not cross-process: jobs are lost on exit and there's
> no separate worker fleet — that's Askr's job on the server. On the desktop this
> is for getting work off the UI thread (exports, uploads, cleanup) with familiar
> ergonomics.

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

## Behavior

- Jobs run **in order**, one at a time, on a background tokio task.
- A handler returns `Result<(), String>`; an error is reported as `failed` on
  `elyra:queue` (no automatic retries).
- A job with no registered handler is reported as `unhandled`.

## Related

- [Events](events.md) — the `elyra:queue` channel. · [Cache](cache.md) · [Storage](storage.md)
- [Sidecar](sidecar.md) — for long-running external processes.
