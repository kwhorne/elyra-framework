//! An ergonomic key-value **cache** facade — the desktop-side counterpart to
//! Askr's shared cache. Same surface as Laravel's `Cache::` (`get` / `put` /
//! `add` / `remember` / `increment` / `forget` / `flush`), so an app feels the
//! same across both worlds; here it's an in-process, TTL-aware store rather than
//! a shared-memory region.
//!
//! Add [`CacheProvider`] to bind it, then resolve `ctx.get::<Cache>()` from
//! commands or reach it from the frontend via `@elyra/runtime`'s `cache`.
//! Values are arbitrary JSON. For durable settings use [`Store`](crate::Store);
//! for real data use the `database` feature.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

/// Default cap on live entries. Reached first by many small values.
pub const DEFAULT_MAX_ENTRIES: usize = 10_000;
/// Default cap on total cached bytes (64 MiB). Reached first by large values.
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;

/// When an entry expires, tracked on **both** clocks.
///
/// `Instant` alone was wrong: on macOS it doesn't advance while the machine
/// sleeps, so a 5-minute TTL survived an overnight suspend. `SystemTime` alone is
/// wrong too, since the wall clock can jump backwards. An entry is expired when
/// *either* deadline has passed.
#[derive(Clone, Copy)]
struct Deadline {
    monotonic: Instant,
    wall: SystemTime,
}

impl Deadline {
    fn in_(ttl: Duration) -> Self {
        Self {
            monotonic: Instant::now() + ttl,
            wall: SystemTime::now() + ttl,
        }
    }

    fn passed(&self) -> bool {
        Instant::now() >= self.monotonic || SystemTime::now() >= self.wall
    }
}

struct Entry {
    bytes: Vec<u8>,
    expires_at: Option<Deadline>,
    /// Logical clock value of the last read/write, for LRU eviction.
    last_used: u64,
}

impl Entry {
    fn is_expired(&self) -> bool {
        self.expires_at.map(|d| d.passed()).unwrap_or(false)
    }
}

/// The cache's guarded state: entries plus the accounting that keeps it bounded.
struct Inner {
    map: HashMap<String, Entry>,
    /// Sum of key + value bytes currently held.
    bytes: usize,
    /// Monotonic counter used as the LRU stamp.
    clock: u64,
    max_entries: usize,
    max_bytes: usize,
}

impl Inner {
    fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            map: HashMap::new(),
            bytes: 0,
            clock: 0,
            max_entries,
            max_bytes,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn size_of(key: &str, bytes: &[u8]) -> usize {
        key.len() + bytes.len()
    }

    fn remove(&mut self, key: &str) -> Option<Entry> {
        let entry = self.map.remove(key)?;
        self.bytes = self.bytes.saturating_sub(Self::size_of(key, &entry.bytes));
        Some(entry)
    }

    fn insert(&mut self, key: String, bytes: Vec<u8>, expires_at: Option<Deadline>) {
        self.remove(&key);
        let last_used = self.tick();
        self.bytes += Self::size_of(&key, &bytes);
        self.map.insert(
            key,
            Entry {
                bytes,
                expires_at,
                last_used,
            },
        );
        self.enforce_limits();
    }

    /// Drop expired entries, then least-recently-used ones, until the caps hold.
    /// Without this the frontend could `cache.put` until the process died.
    fn enforce_limits(&mut self) {
        if self.map.len() <= self.max_entries && self.bytes <= self.max_bytes {
            return;
        }
        self.retain_unexpired();

        while self.map.len() > self.max_entries || self.bytes > self.max_bytes {
            let Some(victim) = self
                .map
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.remove(&victim);
        }
    }

    fn retain_unexpired(&mut self) {
        let mut freed = 0usize;
        self.map.retain(|key, entry| {
            let keep = !entry.is_expired();
            if !keep {
                freed += Self::size_of(key, &entry.bytes);
            }
            keep
        });
        self.bytes = self.bytes.saturating_sub(freed);
    }
}

/// An in-process, thread-safe, TTL-aware cache. Cheap to clone (shared inner).
///
/// Bounded by entry count **and** total bytes (see [`Cache::with_limits`]);
/// when a cap is hit, expired entries go first, then the least recently used.
#[derive(Clone)]
pub struct Cache {
    inner: Arc<Mutex<Inner>>,
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

impl Cache {
    /// A fresh, empty cache with the default limits
    /// ([`DEFAULT_MAX_ENTRIES`] / [`DEFAULT_MAX_BYTES`]).
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_MAX_ENTRIES, DEFAULT_MAX_BYTES)
    }

    /// A cache bounded to `max_entries` live keys and `max_bytes` of data.
    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::new(max_entries, max_bytes))),
        }
    }

    /// How many bytes (keys + values) the cache currently holds.
    pub fn bytes_used(&self) -> usize {
        self.inner.lock().bytes
    }

    /// How many live entries the cache currently holds.
    pub fn len(&self) -> usize {
        self.inner.lock().map.len()
    }

    /// Whether the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Fetch the raw bytes for a key (or `None` if missing/expired).
    pub fn get_raw(&self, key: &str) -> Option<Vec<u8>> {
        let mut inner = self.inner.lock();
        let expired = match inner.map.get(key) {
            Some(entry) => entry.is_expired(),
            None => return None,
        };
        if expired {
            inner.remove(key);
            return None;
        }
        let stamp = inner.tick();
        let entry = inner.map.get_mut(key)?;
        entry.last_used = stamp; // keep hot keys away from the LRU victim slot
        Some(entry.bytes.clone())
    }

    /// Fetch a value (or `None` if missing/expired).
    pub fn get(&self, key: &str) -> Option<Value> {
        self.get_raw(key)
            .and_then(|b| serde_json::from_slice(&b).ok())
    }

    /// Store raw bytes with an optional time-to-live (`None` = forever).
    pub fn put_raw(&self, key: impl Into<String>, bytes: Vec<u8>, ttl: Option<Duration>) {
        self.inner
            .lock()
            .insert(key.into(), bytes, ttl.map(Deadline::in_));
    }

    /// Store a value with an optional time-to-live (`None` = forever).
    pub fn put(&self, key: impl Into<String>, value: impl Into<Value>, ttl: Option<Duration>) {
        let bytes = serde_json::to_vec(&value.into()).unwrap_or_default();
        self.put_raw(key, bytes, ttl);
    }

    /// Store only if the key is absent (atomic). Returns whether it was stored.
    /// Store raw bytes only if the key is absent (atomic). Returns whether stored.
    pub fn add_raw(&self, key: impl Into<String>, bytes: Vec<u8>, ttl: Option<Duration>) -> bool {
        let key = key.into();
        let mut inner = self.inner.lock();
        let occupied = inner
            .map
            .get(&key)
            .map(|e| !e.is_expired())
            .unwrap_or(false);
        if occupied {
            return false;
        }
        inner.insert(key, bytes, ttl.map(Deadline::in_));
        true
    }

    /// Store only if the key is absent (atomic). Returns whether it was stored.
    pub fn add(
        &self,
        key: impl Into<String>,
        value: impl Into<Value>,
        ttl: Option<Duration>,
    ) -> bool {
        self.add_raw(
            key,
            serde_json::to_vec(&value.into()).unwrap_or_default(),
            ttl,
        )
    }

    /// Whether a live value exists.
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Remove a key. Returns whether it existed.
    pub fn forget(&self, key: &str) -> bool {
        self.inner.lock().remove(key).is_some()
    }

    /// Atomically increment `key` **only if** it is below `max`, returning the new
    /// value (or `None` when the limit is already reached).
    ///
    /// One lock for the whole check-and-increment, so concurrent callers can't
    /// both pass the check and overshoot the limit (the rate limiter's race).
    pub fn increment_if_below(&self, key: &str, max: i64, ttl: Option<Duration>) -> Option<i64> {
        let mut inner = self.inner.lock();
        let live = inner.map.get(key).filter(|e| !e.is_expired());
        let current = live
            .and_then(|e| serde_json::from_slice::<Value>(&e.bytes).ok())
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if current >= max {
            return None;
        }
        let next = current + 1;
        // A fresh counter gets the TTL; an existing one keeps its own deadline.
        let expires_at = match inner.map.get(key).filter(|e| !e.is_expired()) {
            Some(entry) => entry.expires_at,
            None => ttl.map(Deadline::in_),
        };
        let bytes = serde_json::to_vec(&Value::from(next)).unwrap_or_default();
        inner.insert(key.to_string(), bytes, expires_at);
        Some(next)
    }

    /// Atomically add `delta` to an integer value (starting from 0), returning
    /// the new value. Non-integer values are treated as 0.
    pub fn increment(&self, key: &str, delta: i64) -> i64 {
        let mut inner = self.inner.lock();
        let current = match inner.map.get(key) {
            Some(e) if !e.is_expired() => serde_json::from_slice::<Value>(&e.bytes)
                .ok()
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            _ => 0,
        };
        let next = current + delta;
        // Preserve any existing TTL.
        let expires_at = inner.map.get(key).and_then(|e| e.expires_at);
        let bytes = serde_json::to_vec(&Value::from(next)).unwrap_or_default();
        inner.insert(key.to_string(), bytes, expires_at);
        next
    }

    /// Subtract `delta` (see [`increment`](Cache::increment)).
    pub fn decrement(&self, key: &str, delta: i64) -> i64 {
        self.increment(key, -delta)
    }

    /// Empty the cache.
    pub fn flush(&self) {
        let mut inner = self.inner.lock();
        inner.map.clear();
        inner.bytes = 0;
    }

    /// Spawn a background task that periodically drops expired entries, so keys
    /// that are written-with-TTL but never read again don't leak memory. Holds a
    /// `Weak` ref, so it stops once the cache is dropped. No-op outside a tokio
    /// runtime. Started by [`CacheProvider`].
    pub(crate) fn start_sweeper(&self, interval: Duration) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let weak = std::sync::Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(inner) = weak.upgrade() else {
                    break; // cache dropped
                };
                inner.lock().retain_unexpired();
            }
        });
    }

    /// A [`RateLimiter`](crate::ratelimit::RateLimiter) backed by this cache.
    pub fn limiter(&self) -> crate::ratelimit::RateLimiter {
        crate::ratelimit::RateLimiter::new(self.clone())
    }

    /// Drop every expired entry now (what the background sweeper runs).
    pub fn sweep(&self) {
        self.inner.lock().retain_unexpired();
    }

    // --- typed helpers -----------------------------------------------------

    /// Fetch and deserialize into `T`.
    pub fn get_as<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.get(key).and_then(|v| serde_json::from_value(v).ok())
    }

    /// Serialize and store a typed value.
    pub fn put_as<T: Serialize>(&self, key: impl Into<String>, value: &T, ttl: Option<Duration>) {
        if let Ok(v) = serde_json::to_value(value) {
            self.put(key, v, ttl);
        }
    }

    /// Return the cached value for `key`, or compute + store it first.
    /// The desktop-side `Cache::remember`.
    pub fn remember<T, F>(&self, key: &str, ttl: Option<Duration>, compute: F) -> T
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> T,
    {
        if let Some(hit) = self.get_as::<T>(key) {
            return hit;
        }
        let value = compute();
        self.put_as(key, &value, ttl);
        value
    }
}

/// Conformance to the shared [`substrate_core::Cache`] contract, so generic
/// code can treat this like the Askr/Laravel cache. Values are opaque bytes.
impl substrate_core::Cache for Cache {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.get_raw(key)
    }
    fn put(&self, key: &str, value: &[u8], ttl: Option<Duration>) {
        self.put_raw(key, value.to_vec(), ttl);
    }
    fn add(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> bool {
        self.add_raw(key, value.to_vec(), ttl)
    }
    fn forget(&self, key: &str) -> bool {
        Cache::forget(self, key)
    }
    fn increment(&self, key: &str, delta: i64) -> i64 {
        Cache::increment(self, key, delta)
    }
    fn flush(&self) {
        Cache::flush(self)
    }
}

/// A [`Provider`](crate::Provider) that binds a [`Cache`] into the container.
///
/// Bounded by default ([`DEFAULT_MAX_ENTRIES`] / [`DEFAULT_MAX_BYTES`]); use
/// [`with_limits`](CacheProvider::with_limits) to change the caps.
///
/// ```no_run
/// use elyra::App;
/// use elyra::cache::CacheProvider;
/// App::new()
///     .provider(CacheProvider::with_limits(5_000, 32 * 1024 * 1024))
///     .run()
///     .unwrap();
/// // in a #[command]: ctx.get::<elyra::cache::Cache>().remember(...)
/// ```
pub struct CacheProvider {
    max_entries: usize,
    max_bytes: usize,
}

impl Default for CacheProvider {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl CacheProvider {
    /// The provider with default limits (same as `CacheProvider`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Bound the cache to `max_entries` keys and `max_bytes` of data.
    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
        }
    }
}

impl crate::Provider for CacheProvider {
    fn register(&self, container: &mut crate::Container) {
        let cache = Cache::with_limits(self.max_entries, self.max_bytes);
        cache.start_sweeper(std::time::Duration::from_secs(60));
        container.bind(cache);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_forget_and_add() {
        let cache = Cache::new();
        assert!(cache.get("k").is_none());
        cache.put("k", "v", None);
        assert_eq!(cache.get("k"), Some(Value::from("v")));
        assert!(cache.has("k"));
        assert!(!cache.add("k", "other", None)); // already present
        assert!(cache.forget("k"));
        assert!(cache.add("k", "fresh", None)); // now absent
    }

    #[test]
    fn sweep_drops_expired_entries() {
        let cache = Cache::new();
        cache.put("temp", 1, Some(Duration::from_millis(0)));
        cache.put("forever", 2, None);
        std::thread::sleep(Duration::from_millis(5));
        // Not read again, so only the sweeper reclaims "temp".
        assert_eq!(cache.len(), 2);
        cache.sweep();
        assert_eq!(cache.len(), 1);
        assert!(cache.get("forever").is_some());
    }

    #[test]
    fn ttl_expires() {
        let cache = Cache::new();
        cache.put("k", 1, Some(Duration::from_millis(0)));
        std::thread::sleep(Duration::from_millis(5));
        assert!(cache.get("k").is_none());
    }

    #[test]
    fn increment_counts_from_zero_and_persists() {
        let cache = Cache::new();
        assert_eq!(cache.increment("hits", 1), 1);
        assert_eq!(cache.increment("hits", 2), 3);
        assert_eq!(cache.decrement("hits", 1), 2);
    }

    #[test]
    fn remember_computes_once() {
        use std::cell::Cell;
        let cache = Cache::new();
        let calls = Cell::new(0);
        assert_eq!(
            cache.remember("x", None, || {
                calls.set(calls.get() + 1);
                42i32
            }),
            42
        );
        assert_eq!(
            cache.remember("x", None, || {
                calls.set(calls.get() + 1);
                42i32
            }),
            42
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn entry_count_is_bounded_by_lru_eviction() {
        // Regression: an unbounded cache let the frontend `put` until OOM.
        let cache = Cache::with_limits(3, DEFAULT_MAX_BYTES);
        for i in 0..3 {
            cache.put(format!("k{i}"), i, None);
        }
        // Touch k0 so it isn't the least-recently-used any more.
        assert!(cache.get("k0").is_some());
        cache.put("k3", 3, None);

        assert_eq!(cache.len(), 3);
        assert!(cache.get("k1").is_none(), "the LRU entry must be evicted");
        assert!(cache.get("k0").is_some());
        assert!(cache.get("k3").is_some());
    }

    #[test]
    fn byte_budget_is_enforced_and_tracked() {
        let cache = Cache::with_limits(1000, 256);
        assert_eq!(cache.bytes_used(), 0);
        for i in 0..20 {
            cache.put_as(format!("blob{i}"), &"x".repeat(64), None);
        }
        assert!(cache.bytes_used() <= 256, "bytes = {}", cache.bytes_used());
        assert!(cache.len() < 20);

        // Accounting is released again on forget/flush.
        cache.flush();
        assert_eq!(cache.bytes_used(), 0);
    }

    #[test]
    fn forget_releases_the_byte_budget() {
        let cache = Cache::new();
        cache.put_as("big", &"y".repeat(1000), None);
        let used = cache.bytes_used();
        assert!(used > 1000);
        assert!(cache.forget("big"));
        assert_eq!(cache.bytes_used(), 0);
    }

    #[test]
    fn ttl_uses_the_wall_clock_too() {
        // A TTL must expire even if the monotonic clock stalls (macOS sleep).
        let cache = Cache::new();
        let past = Deadline {
            monotonic: Instant::now() + Duration::from_secs(3600),
            wall: SystemTime::now() - Duration::from_secs(1),
        };
        cache
            .inner
            .lock()
            .insert("slept".into(), b"1".to_vec(), Some(past));
        assert!(cache.get("slept").is_none());
    }

    #[test]
    fn increment_if_below_is_atomic_and_caps() {
        let cache = Cache::new();
        assert_eq!(cache.increment_if_below("hits", 2, None), Some(1));
        assert_eq!(cache.increment_if_below("hits", 2, None), Some(2));
        assert_eq!(cache.increment_if_below("hits", 2, None), None);
        assert_eq!(cache.get_as::<i64>("hits"), Some(2));
    }

    #[test]
    fn typed_roundtrip() {
        let cache = Cache::new();
        cache.put_as("nums", &vec![1, 2, 3], None);
        assert_eq!(cache.get_as::<Vec<i32>>("nums"), Some(vec![1, 2, 3]));
    }
}
