# Middleware

Middleware wraps command dispatch, forming an onion around the command call —
cross-cutting concerns (logging, timing, auth, rate limiting) live here instead
of in every command. It's Elyra's counterpart to Laravel's HTTP middleware.

## The trait

```rust
use elyra::{command::BoxFuture, CommandRequest, Ctx, Middleware, Next, Result};

struct Timing;

impl Middleware for Timing {
    fn handle(&self, ctx: Ctx, req: CommandRequest, next: Next)
        -> BoxFuture<'static, Result<Vec<u8>>>
    {
        Box::pin(async move {
            let name = req.name.clone();
            let started = std::time::Instant::now();
            let out = next.run(ctx, req).await;   // continue the pipeline
            eprintln!("cmd {name} took {:?}", started.elapsed());
            out
        })
    }
}
```

Register it (outermost-first — the first added wraps the rest):

```rust
App::new().middleware(Timing).commands(commands![..]).run();
```

## Types

- **`CommandRequest`** — `{ name: String, args: Vec<u8> }`. `args` is the raw
  MessagePack body (opaque here, but available to inspect or short-circuit).
- **`Next`** — the continuation. Call `next.run(ctx, req)` to proceed to the next
  middleware, or — at the end of the chain — the command itself. Not calling it
  short-circuits (you return your own `Result<Vec<u8>>`).
- Middleware must be `Send + Sync + 'static` (it runs on tokio).

## Short-circuiting

```rust
fn handle(&self, ctx: Ctx, req: CommandRequest, next: Next)
    -> BoxFuture<'static, Result<Vec<u8>>>
{
    Box::pin(async move {
        if !authorized(&req) {
            return Err(elyra::Error::command("unauthorized"));
        }
        next.run(ctx, req).await
    })
}
```

An `Err` becomes an error response; the frontend promise rejects with a
`CommandError` (same as a fallible command).

## Ordering

Middleware runs in registration order, outermost first:

```rust
App::new()
    .middleware(Logging)   // outermost — sees the request first, response last
    .middleware(Auth)      // inner
    .commands(commands![..]);
```

## Related

- [Commands](commands.md)
- [Architecture — request lifecycle](architecture.md#request-lifecycle-a-command)
