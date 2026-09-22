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

## Per-command middleware

Global middleware wraps every command. For middleware that only some commands
need — an auth check, an audit log — register it under a **name** and let the
command ask for it (Laravel's middleware aliases):

```rust
App::new()
    .middleware(Timing)                                 // global: every command
    .middleware_alias("auth", RequireSession)
    .middleware_alias("audit", AuditLog::new())
    .middleware_group("admin", ["auth", "audit"])       // several at once
    .commands(commands![delete_workspace, list_workspaces]);

#[command(middleware = ["admin"])]
async fn delete_workspace(ctx: Ctx, id: i64) -> Result<()> { /* … */ }

#[command]                                              // global middleware only
async fn list_workspaces(ctx: Ctx) -> Result<Vec<Workspace>> { /* … */ }
```

- **Order:** global middleware runs outermost; then the command's own, in the
  order it lists them, with groups expanded in place. `delete_workspace` above
  runs `Timing` → `RequireSession` → `AuditLog` → the command.
- **Groups** may contain aliases or other groups. A middleware reached twice (a
  group plus its member listed again) runs once, at its first position. A group
  cycle is an error.
- **A single name** works too: `#[command(middleware = "auth")]`.
- **Unknown names stop the app at startup** — `invalid middleware wiring: command
  `delete_workspace` uses middleware `auht`, which is not registered`. A misspelt
  name must never mean the command runs without its auth check.

Per-command middleware and [abilities](security.md#4-per-command-abilities)
solve different problems: an ability decides whether the **webview** may call a
command at all, while middleware wraps **every** call, from the frontend or from
Rust (`TestApp`, another command through the registry).

## Related

- [Commands](commands.md)
- [Architecture — request lifecycle](architecture.md#request-lifecycle-a-command)
