//! A rate limiter over the [`Cache`](crate::Cache) — the desktop counterpart to
//! Laravel's `RateLimiter`. Count attempts against a key that expires after a
//! decay window; throttle when the limit is hit.
//!
//! Get one from a cache: `ctx.get::<Cache>().limiter()` (needs `CacheProvider`).

use std::time::Duration;

use crate::cache::Cache;

/// Counts attempts per key against a cache-backed, self-expiring counter.
#[derive(Clone)]
pub struct RateLimiter {
    cache: Cache,
}

impl RateLimiter {
    /// Build a limiter backed by `cache`.
    pub fn new(cache: Cache) -> Self {
        Self { cache }
    }

    /// Attempts recorded for `key` so far.
    pub fn attempts(&self, key: &str) -> i64 {
        self.cache.get_as::<i64>(key).unwrap_or(0)
    }

    /// Whether `key` has reached `max` attempts.
    pub fn too_many_attempts(&self, key: &str, max: i64) -> bool {
        self.attempts(key) >= max
    }

    /// Attempts left before hitting `max` (never negative).
    pub fn remaining(&self, key: &str, max: i64) -> i64 {
        (max - self.attempts(key)).max(0)
    }

    /// Record a hit for `key`, returning the new count. The counter expires
    /// `decay` after its first hit.
    pub fn hit(&self, key: &str, decay: Duration) -> i64 {
        // Seed with the TTL only if absent (atomic); increment preserves it.
        self.cache.add(key, 0, Some(decay));
        self.cache.increment(key, 1)
    }

    /// Reset the counter for `key`.
    pub fn clear(&self, key: &str) {
        self.cache.forget(key);
    }

    /// Run `callback` if under `max` attempts (recording a hit), else return
    /// `None` (throttled). Mirrors Laravel's `RateLimiter::attempt`.
    ///
    /// The check and the increment happen under **one** cache lock
    /// (`Cache::increment_if_below`), so two concurrent callers can't both see
    /// "under the limit" and push the counter past `max`.
    pub fn attempt<F, R>(&self, key: &str, max: i64, decay: Duration, callback: F) -> Option<R>
    where
        F: FnOnce() -> R,
    {
        self.cache
            .increment_if_below(key, max, Some(decay))
            .map(|_| callback())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_throttles() {
        let limiter = RateLimiter::new(Cache::new());
        let decay = Duration::from_secs(60);
        assert_eq!(limiter.attempts("login:ada"), 0);
        assert_eq!(limiter.remaining("login:ada", 3), 3);

        assert_eq!(limiter.hit("login:ada", decay), 1);
        assert_eq!(limiter.hit("login:ada", decay), 2);
        assert!(!limiter.too_many_attempts("login:ada", 3));
        assert_eq!(limiter.hit("login:ada", decay), 3);
        assert!(limiter.too_many_attempts("login:ada", 3));
        assert_eq!(limiter.remaining("login:ada", 3), 0);

        limiter.clear("login:ada");
        assert_eq!(limiter.attempts("login:ada"), 0);
    }

    #[test]
    fn attempt_is_atomic_across_threads() {
        // Regression: check-then-increment let concurrent callers overshoot `max`.
        use std::sync::atomic::{AtomicI64, Ordering};
        use std::sync::Arc;

        let limiter = Arc::new(RateLimiter::new(Cache::new()));
        let allowed = Arc::new(AtomicI64::new(0));
        let max = 50;

        let mut threads = Vec::new();
        for _ in 0..8 {
            let limiter = limiter.clone();
            let allowed = allowed.clone();
            threads.push(std::thread::spawn(move || {
                for _ in 0..50 {
                    if limiter
                        .attempt("burst", max, Duration::from_secs(60), || ())
                        .is_some()
                    {
                        allowed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(allowed.load(Ordering::Relaxed), max);
        assert_eq!(limiter.attempts("burst"), max);
    }

    #[test]
    fn attempt_runs_until_limited() {
        let limiter = RateLimiter::new(Cache::new());
        let decay = Duration::from_secs(60);
        let mut ran = 0;
        for _ in 0..5 {
            if limiter
                .attempt("send", 2, decay, || {
                    ran += 1;
                })
                .is_none()
            {
                break;
            }
        }
        assert_eq!(ran, 2); // allowed twice, then throttled
    }
}
