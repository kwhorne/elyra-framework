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

## Related

- [Frontend runtime](frontend-runtime.md) — `channel()` details
- [System tray](tray.md) — tray menu clicks arrive on the `"tray"` channel
