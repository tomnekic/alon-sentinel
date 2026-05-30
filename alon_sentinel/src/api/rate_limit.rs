use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use lru::LruCache;

#[derive(Clone)]
pub struct AuthRateLimiter {
    inner: Arc<AuthRateLimiterInner>,
}

struct AuthRateLimiterInner {
    max_requests: usize,
    window: Duration,
    entries: Mutex<HashMap<AuthRateLimitKey, AuthRateLimitEntry>>,
    request_counter: AtomicUsize,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct AuthRateLimitKey {
    bucket: &'static str,
    ip_address: String,
}

struct AuthRateLimitEntry {
    window_started_at: Instant,
    request_count: usize,
}

impl AuthRateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            inner: Arc::new(AuthRateLimiterInner {
                max_requests,
                window,
                entries: Mutex::new(HashMap::new()),
                request_counter: AtomicUsize::new(0),
            }),
        }
    }

    pub fn check(&self, bucket: &'static str, ip_address: &str) -> bool {
        let now = Instant::now();
        let key = AuthRateLimitKey {
            bucket,
            ip_address: ip_address.to_string(),
        };
        let mut entries = self
            .inner
            .entries
            .lock()
            .expect("auth rate limiter entries mutex should not be poisoned");

        if self
            .inner
            .request_counter
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(256)
        {
            let window = self.inner.window;
            entries.retain(|_, entry| now.duration_since(entry.window_started_at) < window);
        }

        let entry = entries.entry(key).or_insert_with(|| AuthRateLimitEntry {
            window_started_at: now,
            request_count: 0,
        });

        if now.duration_since(entry.window_started_at) >= self.inner.window {
            entry.window_started_at = now;
            entry.request_count = 0;
        }

        if entry.request_count >= self.inner.max_requests {
            return false;
        }

        entry.request_count += 1;
        true
    }
}

#[derive(Clone)]
pub struct StatusPageCache {
    inner: Arc<Mutex<LruCache<String, StatusPageCacheEntry>>>,
    ttl: Duration,
}

struct StatusPageCacheEntry {
    bytes: Vec<u8>,
    cached_at: Instant,
}

impl StatusPageCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(capacity).expect("cache capacity must be non-zero"),
            ))),
            ttl,
        }
    }

    pub fn get(&self, slug: &str) -> Option<Vec<u8>> {
        let mut cache = self
            .inner
            .lock()
            .expect("status page cache mutex should not be poisoned");
        let entry = cache.get(slug)?;
        if entry.cached_at.elapsed() < self.ttl {
            Some(entry.bytes.clone())
        } else {
            None
        }
    }

    pub fn set(&self, slug: String, bytes: Vec<u8>) {
        let mut cache = self
            .inner
            .lock()
            .expect("status page cache mutex should not be poisoned");
        cache.put(
            slug,
            StatusPageCacheEntry {
                bytes,
                cached_at: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::AuthRateLimiter;
    use std::{thread::sleep, time::Duration};

    #[test]
    fn rate_limiter_blocks_after_max_requests_per_bucket_and_ip() {
        let limiter = AuthRateLimiter::new(2, Duration::from_secs(60));

        assert!(limiter.check("token", "127.0.0.1"));
        assert!(limiter.check("token", "127.0.0.1"));
        assert!(!limiter.check("token", "127.0.0.1"));
        assert!(limiter.check("token", "127.0.0.2"));
        assert!(limiter.check("admin_login", "127.0.0.1"));
    }

    #[test]
    fn rate_limiter_resets_after_window_elapses() {
        let limiter = AuthRateLimiter::new(1, Duration::from_millis(10));

        assert!(limiter.check("token", "127.0.0.1"));
        assert!(!limiter.check("token", "127.0.0.1"));
        sleep(Duration::from_millis(15));
        assert!(limiter.check("token", "127.0.0.1"));
    }
}
