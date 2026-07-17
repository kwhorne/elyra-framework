# Shared substrate (`substrate-core`)

The [`substrate-core`](../substrate) crate is the **contract** behind the "one
ecosystem" facades. It defines backend-agnostic `Cache`, `Storage`, and `Queue`
traits — the same operations you reach for on the Elyra (Rust + Svelte desktop)
side and the [Askr](https://github.com/kwhorne/askr) (Rust PHP server, Laravel)
side — so the two worlds share a single, verifiable contract while each keeps a
backend that fits its environment.

```
substrate-core            traits: Cache · Storage · Queue  (+ Error/Result)
   ├── Elyra   implements them over local backends  (this repo)
   └── Askr    implements them over its server substrate
```

## The contract

Tiny, dependency-free, `std`-only. Values are **opaque bytes** — the traits fix
the *operations and semantics*, not a cross-language value encoding (a
PHP-serialized value and a JSON one differ), so each side picks its own encoding
on top.

```rust
pub trait Cache {
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    fn put(&self, key: &str, value: &[u8], ttl: Option<Duration>);
    fn add(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> bool;
    fn has(&self, key: &str) -> bool { /* default */ }
    fn forget(&self, key: &str) -> bool;
    fn increment(&self, key: &str, delta: i64) -> i64;
    fn decrement(&self, key: &str, delta: i64) -> i64 { /* default */ }
    fn flush(&self);
}

pub trait Storage {
    fn put(&self, path: &str, contents: &[u8]) -> Result<()>;
    fn get(&self, path: &str) -> Result<Vec<u8>>;
    fn exists(&self, path: &str) -> bool;
    fn delete(&self, path: &str) -> Result<()>;
    fn size(&self, path: &str) -> Result<u64>;
    fn files(&self, dir: &str) -> Result<Vec<String>>;
}

pub trait Queue {
    fn push(&self, job: &str, payload: &[u8]);
}
```

## Elyra's conformance

Elyra's facades implement these traits (re-exported as `elyra::substrate`), so
generic code can be written against the contract:

```rust
use elyra::substrate::Cache as CacheContract;

fn warm(cache: &impl CacheContract) {
    cache.put("ready", b"true", None);
}
// works with elyra::cache::Cache — and with any Askr-side Cache
```

The ergonomic [`Cache`](cache.md) (JSON values, `remember`, typed helpers),
[`Storage`](storage.md), and [`Queue`](queue.md) APIs are sugar layered on top of
the same byte-level contract; `Cache` stores bytes internally, so
`substrate` `get`/`put` round-trip losslessly. Conformance is verified in
`framework/tests/substrate.rs`.

## Backends per world

| Facade | Elyra | Askr / Laravel |
| --- | --- | --- |
| `Cache` | in-process TTL store | shared-memory region |
| `Storage` | jailed local disk | local / S3 disk |
| `Queue` | in-process background jobs | supervised worker fleet |

## Consuming it from Askr (no crates.io needed)

`substrate-core` lives in this repo, so the Askr side depends on it directly as a
**git dependency** pinned to a release tag — no crates.io publishing required:

```toml
# Askr's Cargo.toml
substrate-core = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.5.1" }
```

Cargo resolves the `substrate-core` package from within this workspace. Askr then
implements the same `Cache` / `Storage` / `Queue` traits over its server
substrate, and both worlds share one source of truth for the API shape.

Tradeoff: the contract's version tracks Elyra's release tags, so bump the pin
when the traits change. Publishing to crates.io only becomes worthwhile if a
third party (outside Elyra + Askr) needs the contract.

## Design rule

Keep `substrate-core` **small and backend-agnostic**. It must express *what*
(get/put/ttl, blob I/O, enqueue), never *how* — no fork, shared-memory, or
filesystem assumptions — or it would lock one world out. New operations are
added only when both worlds can implement them.

## Related

- [Cache](cache.md) · [Storage](storage.md) · [Queue](queue.md)
