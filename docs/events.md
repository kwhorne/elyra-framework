# Events — the EventBus

The `EventBus` is Elyra's Broadcasting: Rust pushes events to the frontend,
batched per flush. Rust owns the state; the frontend subscribes to changes
instead of polling.

## Emitting (Rust)

The bus is created by `App` and bound in the container, so any command,
provider, or task can resolve it:

```rust
#[command]
async fn tick(ctx: Ctx) {
    ctx.get::<EventBus>().emit("tick", &42u32).ok();
}
```

`emit<T: Serialize>(channel, &value)` is non-blocking and callable from any
thread. To emit from `main` or a background thread, grab a clone before running:

```rust
let app = App::new().commands(commands![..]);
let bus = app.events();                       // a clone of the bus
std::thread::spawn(move || { bus.emit("tick", &1u32).ok(); });
app.run()?;
```

## Subscribing (frontend)

`channel(name)` returns a **Svelte-readable store**, multiplexed over a single
connection:

```svelte
<script>
  import { channel } from "@elyra/runtime";
  const ticks = channel("tick");   // usable as $ticks
</script>
<p>{$ticks}</p>
```

Or subscribe manually:

```ts
const unsubscribe = channel<number>("tick").subscribe((n) => console.log(n));
```

## Transport & batching

Events travel over a **long-poll** of `elyra://localhost/__events`: the shell
holds the request open until events are ready, responds with a MessagePack batch
(`[[channel, value], ...]`), and the frontend immediately reconnects. Binary, no
base64, one connection for all channels. See [wire format](wire-format.md).

Emits accumulate and flush together, so N state changes cost **one** IPC round,
not N. By default there's no artificial delay — the natural response→reconnect
gap coalesces bursts. For sustained, time-spaced streams you can force
frame-level coalescing:

```rust
App::new().batch_window(std::time::Duration::from_millis(8));
```

After ~20s idle the poll returns an empty keep-alive batch and the connection
refreshes.

## Fan-out: one queue per window

Every webview identifies itself with a random client id (`x-elyra-client-id`,
added by `@elyra/runtime`) and gets **its own queue**. An `emit` is fanned out to
all connected windows, so a multi-window app can't lose events — previously one
shared queue meant whichever window polled first took the batch and the others
never saw it.

Events emitted before *any* window has connected are held and delivered to the
first poll (nothing emitted during startup is lost). A window that opens later
does **not** get a replay of what it missed — push current state from a command
instead when a new window needs to catch up.

## Domain events (`Dispatcher`)

The `EventBus` pushes values *out* to the webview. For events *inside* the Rust
side — one part announces that something happened, others react without the
announcer knowing who they are — use the `Dispatcher`: Laravel's `Event::dispatch`
and listeners.

```rust
#[derive(Clone)]
struct OrderShipped { order_id: i64 }

App::new().listen(|e: OrderShipped, ctx: Ctx| async move {
    ctx.get::<dyn Mailer>().send_shipped(e.order_id).await
});

#[command]
async fn ship(ctx: Ctx, order_id: i64) -> Result<()> {
    // … mark it shipped …
    ctx.dispatch(OrderShipped { order_id }).await
}
```

Listeners can also be registered from a provider — Laravel's
`EventServiceProvider` — in `register` or `boot`:

```rust
impl Provider for AuditProvider {
    fn boot(&self, ctx: &Ctx) {
        ctx.get::<Dispatcher>().listen(|e: OrderShipped, ctx: Ctx| async move {
            ctx.get::<AuditLog>().record("shipped", e.order_id)
        });
    }
}
```

- An event is any `Clone + Send + Sync + 'static` type; each listener gets its
  own copy.
- Listeners run **in registration order, one at a time**: `App::listen` calls
  first, then providers in the order they were added.
- **The first error stops the chain** and is returned from `dispatch`, so the
  command that dispatched fails with the listener's message. An event nobody
  listens for is not an error.
- `ctx.dispatch_background(event)` returns immediately and runs the chain on its
  own task; a listener error is logged under `elyra::events` instead.
- A listener may dispatch further events.

### Broadcasting to the frontend

`App::broadcast` forwards every dispatched event to the `EventBus` **and**
declares its channel type for codegen — Laravel's `ShouldBroadcast`, typed end to
end:

```rust
#[derive(Clone, serde::Serialize, specta::Type)]
struct OrderShipped { order_id: i64 }

App::new().broadcast::<OrderShipped>("orders:shipped");
```

```svelte
<script>
  import { channel } from "./bindings";            // the generated, typed helper
  const shipped = channel("orders:shipped");        // typed as OrderShipped
</script>
{#if $shipped}<p>Order {$shipped.order_id} shipped</p>{/if}
```

The Rust side dispatches a domain event and never mentions the webview; the
frontend gets a typed channel.

## Related

- [Frontend runtime](frontend-runtime.md) — `channel()` details
- [System tray](tray.md) — tray menu clicks arrive on the `"tray"` channel
