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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

struct Entry {
    bytes: Vec<u8>,
    expires_at: Option<Instant>,
}

impl Entry {
    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|e| Instant::now() >= e)
            .unwrap_or(false)
    }
}

/// An in-process, thread-safe, TTL-aware cache. Cheap to clone (shared inner).
#[derive(Clone, Default)]
pub struct Cache {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

impl Cache {
    /// A fresh, empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch the raw bytes for a key (or `None` if missing/expired).
    pub fn get_raw(&self, key: &str) -> Option<Vec<u8>> {
        let mut map = self.inner.lock().unwrap();
        match map.get(key) {
            Some(entry) if entry.is_expired() => {
                map.remove(key);
                None
            }
            Some(entry) => Some(entry.bytes.clone()),
            None => None,
        }
    }

    /// Fetch a value (or `None` if missing/expired).
    pub fn get(&self, key: &str) -> Option<Value> {
        self.get_raw(key)
            .and_then(|b| serde_json::from_slice(&b).ok())
    }

    /// Store raw bytes with an optional time-to-live (`None` = forever).
    pub fn put_raw(&self, key: impl Into<String>, bytes: Vec<u8>, ttl: Option<Duration>) {
        let entry = Entry {
            bytes,
            expires_at: ttl.map(|d| Instant::now() + d),
        };
        self.inner.lock().unwrap().insert(key.into(), entry);
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
        let mut map = self.inner.lock().unwrap();
        let occupied = map.get(&key).map(|e| !e.is_expired()).unwrap_or(false);
        if occupied {
            return false;
        }
        map.insert(
            key,
            Entry {
                bytes,
                expires_at: ttl.map(|d| Instant::now() + d),
            },
        );
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
        self.inner.lock().unwrap().remove(key).is_some()
    }

    /// Atomically add `delta` to an integer value (starting from 0), returning
    /// the new value. Non-integer values are treated as 0.
    pub fn increment(&self, key: &str, delta: i64) -> i64 {
        let mut map = self.inner.lock().unwrap();
        let current = match map.get(key) {
            Some(e) if !e.is_expired() => serde_json::from_slice::<Value>(&e.bytes)
                .ok()
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            _ => 0,
        };
        let next = current + delta;
        // Preserve any existing TTL.
        let expires_at = map.get(key).and_then(|e| e.expires_at);
        map.insert(
            key.to_string(),
            Entry {
                bytes: serde_json::to_vec(&Value::from(next)).unwrap_or_default(),
                expires_at,
            },
        );
        next
    }

    /// Subtract `delta` (see [`increment`](Cache::increment)).
    pub fn decrement(&self, key: &str, delta: i64) -> i64 {
        self.increment(key, -delta)
    }

    /// Empty the cache.
    pub fn flush(&self) {
        self.inner.lock().unwrap().clear();
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
                let now = Instant::now();
                inner
                    .lock()
                    .unwrap()
                    .retain(|_, e| e.expires_at.map(|x| x > now).unwrap_or(true));
            }
        });
    }

    /// Drop every expired entry now (what the background sweeper runs).
    pub fn sweep(&self) {
        let now = Instant::now();
        self.inner
            .lock()
            .unwrap()
            .retain(|_, e| e.expires_at.map(|x| x > now).unwrap_or(true));
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
/// ```no_run
/// use elyra::App;
/// use elyra::cache::CacheProvider;
/// App::new().provider(CacheProvider).run().unwrap();
/// // in a #[command]: ctx.get::<elyra::cache::Cache>().remember(...)
/// ```
pub struct CacheProvider;

impl crate::Provider for CacheProvider {
    fn register(&self, container: &mut crate::Container) {
        let cache = Cache::new();
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
        assert_eq!(cache.inner.lock().unwrap().len(), 2);
        cache.sweep();
        assert_eq!(cache.inner.lock().unwrap().len(), 1);
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
    fn typed_roundtrip() {
        let cache = Cache::new();
        cache.put_as("nums", &vec![1, 2, 3], None);
        assert_eq!(cache.get_as::<Vec<i32>>("nums"), Some(vec![1, 2, 3]));
    }
}
