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
    value: Value,
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

    /// Fetch a value (or `None` if missing/expired).
    pub fn get(&self, key: &str) -> Option<Value> {
        let mut map = self.inner.lock().unwrap();
        match map.get(key) {
            Some(entry) if entry.is_expired() => {
                map.remove(key);
                None
            }
            Some(entry) => Some(entry.value.clone()),
            None => None,
        }
    }

    /// Store a value with an optional time-to-live (`None` = forever).
    pub fn put(&self, key: impl Into<String>, value: impl Into<Value>, ttl: Option<Duration>) {
        let entry = Entry {
            value: value.into(),
            expires_at: ttl.map(|d| Instant::now() + d),
        };
        self.inner.lock().unwrap().insert(key.into(), entry);
    }

    /// Store only if the key is absent (atomic). Returns whether it was stored.
    pub fn add(
        &self,
        key: impl Into<String>,
        value: impl Into<Value>,
        ttl: Option<Duration>,
    ) -> bool {
        let key = key.into();
        let mut map = self.inner.lock().unwrap();
        let occupied = map.get(&key).map(|e| !e.is_expired()).unwrap_or(false);
        if occupied {
            return false;
        }
        map.insert(
            key,
            Entry {
                value: value.into(),
                expires_at: ttl.map(|d| Instant::now() + d),
            },
        );
        true
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
            Some(e) if !e.is_expired() => e.value.as_i64().unwrap_or(0),
            _ => 0,
        };
        let next = current + delta;
        // Preserve any existing TTL.
        let expires_at = map.get(key).and_then(|e| e.expires_at);
        map.insert(
            key.to_string(),
            Entry {
                value: Value::from(next),
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
        container.bind(Cache::new());
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
