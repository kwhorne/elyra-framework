//! A small persistent key-value store.
//!
//! Backed by a `settings.json` file in the OS config directory (keyed by the
//! [About](crate::AboutInfo) name). Bound into the container automatically, so
//! it's reachable from commands (`ctx.get::<Store>()`) and from the frontend via
//! `@elyra/runtime`'s `store`.
//!
//! Values are arbitrary JSON. For durable settings use this; for real data use
//! the `database` feature.
//!
//! ## Durability
//! Writes are **atomic**: the file is written to `settings.json.tmp` and renamed
//! over the target, so a crash mid-write can't leave a truncated or empty
//! settings file (the previous contents survive). The last good version is also
//! kept as `settings.json.bak` and used automatically if the main file is
//! unreadable.
//!
//! ## Write coalescing
//! `set()` used to serialize the whole map and hit the disk on **every** call,
//! synchronously on the IPC thread — a frontend that persisted on each keystroke
//! wrote the file per keystroke. Writes are now coalesced: the in-memory map
//! updates immediately, and the flush happens on a short debounce (and always on
//! `flush()`/drop).

use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};

/// How long a burst of `set()` calls is coalesced before hitting the disk.
const DEBOUNCE: Duration = Duration::from_millis(250);

struct Inner {
    path: Option<PathBuf>,
    data: Mutex<Map<String, Value>>,
    /// Set while a flush is already scheduled, so a burst schedules one task.
    flush_pending: AtomicBool,
}

impl Inner {
    /// Serialize and atomically replace the file. Returns whether it was written.
    fn persist(&self) -> bool {
        let Some(path) = &self.path else {
            return false;
        };
        let snapshot = self.data.lock().clone();
        let Ok(text) = serde_json::to_string_pretty(&snapshot) else {
            return false;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }

        // Keep the previous good file as a backup before replacing it.
        if path.exists() {
            let _ = std::fs::copy(path, path.with_extension("json.bak"));
        }

        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text.as_bytes()).is_err() {
            return false;
        }
        // rename(2) is atomic within a filesystem: readers see either the old or
        // the new file, never a half-written one.
        if std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        true
    }
}

/// A JSON-backed key-value store, safe to share across threads.
pub struct Store {
    inner: Arc<Inner>,
}

impl Store {
    /// Open (or start) the store for `app`, loading any existing `settings.json`
    /// (falling back to the `.bak` copy if the main file is unreadable).
    pub(crate) fn open(app: &str) -> Store {
        let path = crate::winstate::app_dir(app).map(|d| d.join("settings.json"));
        let data = path.as_deref().and_then(read_map).unwrap_or_default();
        Store {
            inner: Arc::new(Inner {
                path,
                data: Mutex::new(data),
                flush_pending: AtomicBool::new(false),
            }),
        }
    }

    /// Schedule a coalesced flush, or write immediately when there's no runtime
    /// (e.g. a synchronous test or `main` before the runtime starts).
    fn schedule_flush(&self) {
        if self.inner.path.is_none() {
            return;
        }
        if tokio::runtime::Handle::try_current().is_err() {
            self.inner.persist();
            return;
        }
        // Already scheduled: the pending flush will pick up this change too.
        if self.inner.flush_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = self.inner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(DEBOUNCE).await;
            inner.flush_pending.store(false, Ordering::Release);
            let write = inner.clone();
            // Disk IO belongs off the IPC/UI thread.
            let _ = tokio::task::spawn_blocking(move || write.persist()).await;
        });
    }

    /// Write any pending changes to disk now. Called automatically on drop.
    pub fn flush(&self) {
        self.inner.flush_pending.store(false, Ordering::Release);
        self.inner.persist();
    }

    /// The file backing this store, if the config directory could be resolved.
    pub fn path(&self) -> Option<&Path> {
        self.inner.path.as_deref()
    }

    /// Get a value by key.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.inner.data.lock().get(key).cloned()
    }

    /// Set a value. Persisted on a short debounce (see module docs).
    pub fn set(&self, key: impl Into<String>, value: Value) {
        self.inner.data.lock().insert(key.into(), value);
        self.schedule_flush();
    }

    /// Remove a key. Returns whether it existed.
    pub fn delete(&self, key: &str) -> bool {
        let existed = self.inner.data.lock().remove(key).is_some();
        if existed {
            self.schedule_flush();
        }
        existed
    }

    /// A snapshot of every key/value.
    pub fn all(&self) -> Map<String, Value> {
        self.inner.data.lock().clone()
    }

    /// Remove everything (written through immediately \u2014 it's destructive).
    pub fn clear(&self) {
        self.inner.data.lock().clear();
        self.flush();
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // The last owner writes out whatever the debounce hasn't flushed yet.
        if Arc::strong_count(&self.inner) == 1 {
            self.inner.persist();
        }
    }
}

/// Read a JSON object from `path`, falling back to its `.bak` companion.
fn read_map(path: &Path) -> Option<Map<String, Value>> {
    let parse = |p: &Path| {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str::<Map<String, Value>>(&t).ok())
    };
    parse(path).or_else(|| parse(&path.with_extension("json.bak")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (Store, PathBuf) {
        use std::sync::atomic::AtomicU32;
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "elyra-store-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let store = Store {
            inner: Arc::new(Inner {
                path: Some(path.clone()),
                data: Mutex::new(Map::new()),
                flush_pending: AtomicBool::new(false),
            }),
        };
        (store, path)
    }

    #[test]
    fn in_memory_crud_without_a_path() {
        // With no path (unresolved config dir) it still works in memory.
        let store = Store {
            inner: Arc::new(Inner {
                path: None,
                data: Mutex::new(Map::new()),
                flush_pending: AtomicBool::new(false),
            }),
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

    #[test]
    fn writes_are_atomic_and_leave_no_temp_file() {
        let (store, path) = temp_store();
        store.set("theme", Value::from("dark"));
        store.flush();

        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"theme\""));
    }

    #[test]
    fn a_corrupt_file_falls_back_to_the_backup() {
        let (store, path) = temp_store();
        store.set("a", Value::from(1));
        store.flush();
        store.set("b", Value::from(2));
        store.flush(); // now a .bak exists with {"a":1}

        // Simulate a truncated write (what a non-atomic writer could leave).
        std::fs::write(&path, b"{ this is not json").unwrap();
        let reloaded = read_map(&path).expect("must fall back to the backup");
        assert_eq!(reloaded.get("a"), Some(&Value::from(1)));
    }

    #[tokio::test]
    async fn bursts_of_set_coalesce_into_one_write() {
        let (store, path) = temp_store();
        for i in 0..50 {
            store.set(format!("k{i}"), Value::from(i));
        }
        // Nothing on disk yet: the flush is debounced.
        assert!(!path.exists(), "a burst must not write per set()");

        tokio::time::sleep(DEBOUNCE + Duration::from_millis(150)).await;
        assert!(path.exists(), "the debounced flush must land");
        let map = read_map(&path).unwrap();
        assert_eq!(map.len(), 50);
    }

    #[test]
    fn drop_flushes_pending_changes() {
        let (store, path) = temp_store();
        store.set("pending", Value::from(true));
        drop(store);
        let map = read_map(&path).expect("drop must persist");
        assert_eq!(map.get("pending"), Some(&Value::from(true)));
    }
}
