//! # substrate-core
//!
//! The **shared contract** behind the "one ecosystem" facades. It defines
//! backend-agnostic [`Cache`], [`Storage`], and [`Queue`] traits — the same
//! operations an app reaches for on the [Elyra](https://elyracode.com/framework)
//! (Rust + Svelte desktop) side and the
//! [Askr](https://github.com/kwhorne/askr) (Rust PHP server, Laravel) side.
//!
//! The traits express **what** (get/put/ttl, blob I/O, enqueue), never **how**
//! (no shared-memory, fork, or filesystem assumptions leak in), so each world
//! can implement them over its own substrate:
//!
//! | Facade | Elyra | Askr / Laravel |
//! | --- | --- | --- |
//! | [`Cache`] | in-process TTL store | shared-memory region |
//! | [`Storage`] | jailed local disk | local / S3 disk |
//! | [`Queue`] | in-process background jobs | supervised worker fleet |
//!
//! Values are **opaque bytes**: the contract fixes the operations and their
//! semantics, not a cross-language value encoding (a PHP-serialized value and a
//! JSON one differ) — each side chooses its own encoding on top.
//!
//! This crate is intentionally tiny and dependency-free so both ecosystems can
//! depend on it without friction.

#![forbid(unsafe_code)]

use std::time::Duration;

/// A substrate error (portable across backends).
#[derive(Debug, Clone)]
pub struct Error(pub String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error(message.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// A substrate result.
pub type Result<T> = std::result::Result<T, Error>;

/// A byte-oriented key-value cache with TTLs and atomic counters.
///
/// TTL semantics: `Some(d)` expires after `d`; `None` never expires.
pub trait Cache {
    /// Fetch a value (or `None` if missing/expired).
    fn get(&self, key: &str) -> Option<Vec<u8>>;

    /// Store a value with an optional time-to-live.
    fn put(&self, key: &str, value: &[u8], ttl: Option<Duration>);

    /// Store only if absent (atomic). Returns whether it was stored.
    fn add(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> bool;

    /// Whether a live value exists.
    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Remove a key. Returns whether it existed.
    fn forget(&self, key: &str) -> bool;

    /// Atomically add `delta` to an integer value (from 0), returning the new value.
    fn increment(&self, key: &str, delta: i64) -> i64;

    /// Atomically subtract `delta`.
    fn decrement(&self, key: &str, delta: i64) -> i64 {
        self.increment(key, -delta)
    }

    /// Empty the cache.
    fn flush(&self);
}

/// A byte-oriented blob store addressed by relative path.
///
/// Implementations must keep paths confined to their root (reject `..` etc.).
pub trait Storage {
    /// Write bytes (creating parents).
    fn put(&self, path: &str, contents: &[u8]) -> Result<()>;

    /// Read bytes.
    fn get(&self, path: &str) -> Result<Vec<u8>>;

    /// Whether a file exists.
    fn exists(&self, path: &str) -> bool;

    /// Delete a file (a missing file is not an error).
    fn delete(&self, path: &str) -> Result<()>;

    /// File size in bytes.
    fn size(&self, path: &str) -> Result<u64>;

    /// File names directly inside `dir` (non-recursive).
    fn files(&self, dir: &str) -> Result<Vec<String>>;
}

/// A fire-and-forget background job queue. Payloads are opaque bytes; handler
/// registration and execution are the implementation's concern.
pub trait Queue {
    /// Enqueue a named job with a byte payload.
    fn push(&self, job: &str, payload: &[u8]);
}
