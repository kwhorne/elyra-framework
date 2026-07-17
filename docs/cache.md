# Cache

An ergonomic key-value **cache** facade — the desktop-side counterpart to
[Askr](https://github.com/kwhorne/askr)'s shared cache, with the same surface as
Laravel's `Cache::`. One ecosystem: an app reaches for `Cache::remember(...)` in
both worlds; here it's an in-process, TTL-aware store (not a shared-memory
region — a desktop app is a single process).

Add the provider to enable it:

```rust
use elyra::App;
use elyra::cache::CacheProvider;

App::new().provider(CacheProvider).run()?;
```

## Rust API

Resolve it from the container:

```rust
use elyra::cache::Cache;
use std::time::Duration;

#[command]
async fn dashboard(ctx: Ctx) -> Stats {
    let cache = ctx.get::<Cache>();

    // Compute once, reuse for 60s.
    cache.remember("stats", Some(Duration::from_secs(60)), || compute_stats())
}
```

Full surface:

```rust
cache.put("key", "value", Some(Duration::from_secs(300)));  // TTL; None = forever
cache.get("key");                                           // Option<serde_json::Value>
cache.add("once", true, None);                              // set-if-absent (atomic) -> bool
cache.has("key");
cache.forget("key");
cache.increment("hits", 1);                                 // atomic counter -> i64
cache.decrement("hits", 1);
cache.flush();

// Typed helpers (serde):
cache.put_as("user", &user, None);
let user = cache.get_as::<User>("user");
```

## Frontend API (`@elyra/runtime`)

The same cache instance, from Svelte:

```ts
import { cache } from "@elyra/runtime";

await cache.put("theme", "dark", 3600);      // expires in 1h
const theme = await cache.get<string>("theme");
const n = await cache.increment("clicks");
await cache.forget("theme");
```

## Cache vs Store vs Database

| Use | Reach for |
| --- | --- |
| Ephemeral, TTL'd values, counters, memoization | **Cache** (this) |
| Durable app settings (survive restarts) | [`Store`](store.md) |
| Structured, queryable data | [`database`](database.md) |

The cache lives in memory and is cleared when the app exits.

## Related

- [Container & providers](container-and-providers.md) · [Store](store.md)
