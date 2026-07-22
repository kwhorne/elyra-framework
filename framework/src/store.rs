//! A small persistent key-value store.
//!
//! Backed by a `settings.json` file in the OS config directory (keyed by the
//! [About](crate::AboutInfo) name). Bound into the container automatically, so
//! it's reachable from commands (`ctx.get::<Store>()`) and from the frontend via
//! `@elyra/runtime`'s `store`.
//!
//! Values are arbitrary JSON. This is for app settings and small state — not a
//! database (see the `database` feature for that).

use parking_lot::Mutex;
use std::path::PathBuf;

use serde_json::{Map, Value};

/// A JSON-backed key-value store, safe to share across threads.
pub struct Store {
    path: Option<PathBuf>,
    data: Mutex<Map<String, Value>>,
}

impl Store {
    /// Open (or start) the store for `app`, loading any existing `settings.json`.
    pub(crate) fn open(app: &str) -> Store {
        let path = crate::winstate::app_dir(app).map(|d| d.join("settings.json"));
        let data = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Map<String, Value>>(&t).ok())
            .unwrap_or_default();
        Store {
            path,
            data: Mutex::new(data),
        }
    }

    fn persist(&self, data: &Map<String, Value>) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(data) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Get a value by key.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.lock().get(key).cloned()
    }

    /// Set a value, persisting to disk.
    pub fn set(&self, key: impl Into<String>, value: Value) {
        let mut data = self.data.lock();
        data.insert(key.into(), value);
        self.persist(&data);
    }

    /// Remove a key. Returns whether it existed.
    pub fn delete(&self, key: &str) -> bool {
        let mut data = self.data.lock();
        let existed = data.remove(key).is_some();
        if existed {
            self.persist(&data);
        }
        existed
    }

    /// A snapshot of every key/value.
    pub fn all(&self) -> Map<String, Value> {
        self.data.lock().clone()
    }

    /// Remove everything.
    pub fn clear(&self) {
        let mut data = self.data.lock();
        data.clear();
        self.persist(&data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_crud_without_a_path() {
        // With no path (unresolved config dir) it still works in memory.
        let store = Store {
            path: None,
            data: Mutex::new(Map::new()),
        };
        assert!(store.get("k").is_none());
        store.set("k", Value::from(42));
        assert_eq!(store.get("k"), Some(Value::from(42)));
        assert_eq!(store.all().len(), 1);
        assert!(store.delete("k"));
        assert!(!store.delete("k"));
        store.set("a", Value::from("x"));
        store.clear();
        assert_eq!(store.all().len(), 0);
    }
}
