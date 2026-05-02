use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Minimum-spacing rate limiter for the Alpha Vantage HTTP API.
///
/// Alpha Vantage's free tier permits 1 request per second (and 25/day).
/// `acquire()` returns immediately when at least `min_spacing` has elapsed
/// since the last grant; otherwise it sleeps until that gap is available.
/// The mutex is held only long enough to read/write the timestamp; the
/// sleep happens after the guard is dropped so multiple awaiters wake
/// fairly without holding the lock.
pub struct AlphaVantageRateLimiter {
    last_grant: Arc<Mutex<Option<Instant>>>,
    min_spacing: Duration,
    acquire_count: Arc<AtomicU64>,
}

impl AlphaVantageRateLimiter {
    pub fn new(min_spacing: Duration) -> Self {
        Self {
            last_grant: Arc::new(Mutex::new(None)),
            min_spacing,
            acquire_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Default for AV free tier: one request per second.
    pub fn per_second() -> Self {
        Self::new(Duration::from_secs(1))
    }

    pub async fn acquire(&self) {
        loop {
            let sleep_for = {
                let mut guard = self.last_grant.lock().await;
                let now = Instant::now();

                let next_allowed = guard.map(|t| t + self.min_spacing);
                match next_allowed {
                    Some(at) if at > now => at - now,
                    _ => {
                        *guard = Some(now);
                        self.acquire_count.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                }
            };

            tokio::time::sleep(sleep_for).await;
        }
    }

    /// Total number of successful `acquire()` calls since construction.
    /// Used in tests to assert the limiter was actually consulted.
    #[allow(dead_code)]
    pub fn acquire_count(&self) -> u64 {
        self.acquire_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::time::Instant as TokioInstant;

    #[tokio::test]
    async fn first_acquire_is_immediate() {
        let limiter = AlphaVantageRateLimiter::per_second();
        let start = TokioInstant::now();
        limiter.acquire().await;
        assert!(start.elapsed() < Duration::from_millis(50));
        assert_eq!(limiter.acquire_count(), 1);
    }

    #[tokio::test]
    async fn five_back_to_back_acquires_span_at_least_four_seconds() {
        let limiter = AlphaVantageRateLimiter::per_second();
        let start = TokioInstant::now();
        for _ in 0..5 {
            limiter.acquire().await;
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(4000),
            "5 sequential acquires must span >= 4s; got {elapsed:?}"
        );
        assert_eq!(limiter.acquire_count(), 5);
    }

    #[tokio::test]
    async fn concurrent_acquires_are_serialized() {
        let limiter = Arc::new(AlphaVantageRateLimiter::new(Duration::from_millis(200)));
        let start = TokioInstant::now();
        let mut handles = Vec::new();
        for _ in 0..3 {
            let l = Arc::clone(&limiter);
            handles.push(tokio::spawn(async move { l.acquire().await }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(400),
            "3 concurrent acquires at 200ms spacing must span >= 400ms; got {elapsed:?}"
        );
        assert_eq!(limiter.acquire_count(), 3);
    }
}
