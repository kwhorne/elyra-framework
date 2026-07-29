# Rate limiting

A cache-backed rate limiter — the desktop counterpart to Laravel's
`RateLimiter`. Count attempts against a key that expires after a decay window,
and throttle once the limit is hit. Needs the [`Cache`](cache.md)
(`CacheProvider`).

```rust
use elyra::{command, Ctx, Cache};
use std::time::Duration;

#[command]
async fn login(ctx: Ctx, email: String, password: String) -> Result<Session, String> {
    let limiter = ctx.get::<Cache>().limiter();
    let key = format!("login:{email}");

    if limiter.too_many_attempts(&key, 5) {
        return Err("Too many attempts. Try again later.".into());
    }

    match authenticate(&email, &password) {
        Some(session) => { limiter.clear(&key); Ok(session) }
        None => {
            limiter.hit(&key, Duration::from_secs(60)); // 5 tries / minute
            Err("Invalid credentials.".into())
        }
    }
}
```

## API

| Method | Does |
| --- | --- |
| `too_many_attempts(key, max)` | whether `key` reached `max` |
| `hit(key, decay)` | record an attempt; returns the new count |
| `attempts(key)` | attempts so far |
| `remaining(key, max)` | attempts left (never negative) |
| `clear(key)` | reset the counter |
| `attempt(key, max, decay, callback)` | run `callback` if under `max` (recording a hit), else `None` |

`attempt` is the concise form:

```rust
let limiter = ctx.get::<Cache>().limiter();
match limiter.attempt("send-email", 3, Duration::from_secs(60), || send()) {
    Some(result) => result,
    None => return Err("rate limited".into()),
}
```

The counter is a cache entry that expires `decay` after its first hit, so limits
reset automatically. In-process (per app run), like the [cache](cache.md) itself.

`attempt()` does the check **and** the increment under one cache lock
(`Cache::increment_if_below`), so concurrent callers can't both see "under the
limit" and push the counter past `max`.

## Related

- [Cache](cache.md) · [Container & providers](container-and-providers.md)
