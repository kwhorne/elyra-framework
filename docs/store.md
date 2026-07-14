# Settings store

A small persistent key-value store for app settings and light state — always
available, no feature flag. Values are arbitrary JSON, saved to a
`settings.json` file in the OS config directory (keyed by the
[About](about.md) name). For real data, use the [`database`](database.md)
feature instead.

## Frontend API (`@elyra/runtime`)

```ts
import { store } from "@elyra/runtime";

await store.set("theme", "dark");
await store.set("window", { pinned: true, tab: 3 });

const theme = await store.get<string>("theme");   // "dark" | null
const all = await store.all();                     // Record<string, unknown>

await store.delete("theme");
await store.clear();
```

## Rust API

The store is bound in the container, so commands can use it too:

```rust
use elyra::Store;

#[command]
async fn remember(ctx: Ctx, key: String, value: serde_json::Value) {
    ctx.get::<Store>().set(key, value);
}
```

`Store` exposes `get`, `set`, `delete`, `all`, and `clear`. Writes persist
immediately.

## Related

- [Database](database.md) — for structured, queryable data.
- [Windows](windows.md) — `persist_window_state` uses the same config directory.
