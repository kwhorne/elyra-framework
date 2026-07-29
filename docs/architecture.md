# Architecture

Elyra is a Laravel-shaped desktop framework: the same ergonomic building blocks —
an application container, service providers, middleware, a typed client — but
compiled to a single native binary with no runtime interpreter.

## The Laravel map

| Laravel | Elyra |
|---|---|
| Application + container | [`App`](container-and-providers.md) + `Container` (`ctx.get::<T>()`) |
| ServiceProvider | [`Provider`](container-and-providers.md) (`register` / `boot`) |
| routes/web.php | [`commands![...]`](commands.md) |
| Controller action | `#[command] async fn` |
| Middleware | [pipeline](middleware.md) in `CommandRegistry::dispatch` |
| Broadcasting + Echo | [`EventBus`](events.md) + `channel()` store |
| Facades / HTTP client | generated [`api.*`](codegen.md) |
| Eloquent | [`#[derive(Model)]`](models.md) |
| Artisan | [Ratatosk (`rata`)](cli.md) |
| Blade / Livewire | Svelte 5 (runes) |

## Processes and threads

- The **event loop + webview** live on the main thread (required on macOS).
- A separate **multi-thread tokio runtime** owns all IPC work.
- The custom-protocol handler is **asynchronous**: each request is spawned onto
  tokio and responded to from there, so the UI thread never blocks on a command
  or an event long-poll.

```
┌─────────────── main thread ───────────────┐     ┌──── tokio runtime ────┐
│ tao event loop  ──  wry webview (WebKit)   │     │  command dispatch     │
│   └ elyra:// custom protocol handler ──────┼────▶│  middleware pipeline  │
│                                            │◀────┼─  EventBus flushes     │
└────────────────────────────────────────────┘     └───────────────────────┘
```

## Request lifecycle (a command)

1. Frontend calls `invoke("name", ...args)` (or `api.name(...)`).
2. `@elyra/runtime` MessagePack-encodes the args and `fetch`es
   `elyra://localhost/__cmd/name`.
3. The protocol handler spawns the request on tokio.
4. `CommandRegistry::dispatch` runs the [middleware pipeline](middleware.md),
   then the command.
5. `#[command]` decodes the arg tuple, runs your `async fn`, and MessagePack-encodes
   the result (or maps a `Result::Err` to an error response).
6. The bytes are returned; the runtime decodes them into the resolved value.

See [wire format](wire-format.md) for the exact bytes.

## The IPC boundary

The webview is untrusted: anything executing in the page — your code, a dependency,
injected content — can reach `/__*`. Three mechanisms gate it, all enforced in
`shell::route` before dispatch:

1. a random **per-run token** injected into the webview before any page script runs,
2. **origin isolation** — no CORS headers at all in a production build,
3. a **capability model** where destructive routes are opt-in,

plus structural limits on request bodies (size + nesting depth). See
[security](security.md).

## State ownership

Rust owns the state; the frontend is a projection. Instead of one IPC round per
change, the [`EventBus`](events.md) accumulates events and flushes them as a
single batch to a long-poll the frontend holds open — binary, no base64.

## Events: one queue per window

`EventBus` keeps a queue **per connected webview**, keyed on a client id the runtime
sends. An `emit` fans out to every window; a single shared queue meant whichever
window polled first consumed the batch and the others silently lost it. Events
emitted before any window connects are held for the first poll.

## Crates

```
framework/   elyra          App, Container/Ctx, Command, EventBus, shell (tao+wry),
                            security policy, windows, tray, updater, codegen,
                            Log/Config/Secrets/testing
macros/      elyra-macros   #[command], #[derive(Model)]
database/    elyra-db       Database (sqlx Any), schema builder, migrations, models
                            — no GUI deps, so the CLI can use it without tao/wry
ai/          elyra-ai       the AI SDK (agents, tools, embeddings, …)
substrate/   substrate-core the shared Cache/Storage/Queue contracts
ratatosk/    ratatosk       the `rata` CLI
runtime/     @elyra/runtime invoke(), channel(), the generated api.*
```

`elyra-db` is deliberately GUI-free so `rata migrate` (and any headless tool) can
drive the database without pulling in the windowing stack.

## Performance principles

1. **Binary IPC** — MessagePack over the custom protocol via `fetch()`. No JSON
   in the hot path; streaming and `ArrayBuffer` for free.
2. **Rust owns state** — diffs pushed via the `EventBus`, batched per flush.
3. **Assets from memory** — `rust-embed` + protocol handler, no disk I/O at start.
4. **Never block the UI thread** — every command runs on tokio.
