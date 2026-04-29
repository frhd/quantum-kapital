use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Sliding-window rate limiter intended for the IBKR historical-bars
/// endpoint (~6 requests/minute is the documented soft cap).
///
/// `acquire()` returns immediately when there's capacity, otherwise it
/// awaits until the oldest in-window request "ages out" (60s window).
/// The mutex guard is always dropped before sleeping, so multiple awaiters
/// can wake up and contend cleanly.
pub struct HistoricalRateLimiter {
    inner: Arc<Mutex<VecDeque<Instant>>>,
    max_per_minute: u32,
    acquire_count: Arc<AtomicU64>,
}

impl HistoricalRateLimiter {
    pub fn new(max_per_minute: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(
                max_per_minute as usize + 1,
            ))),
            max_per_minute,
            acquire_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn acquire(&self) {
        let window = Duration::from_secs(60);
        loop {
            let sleep_for = {
                let mut guard = self.inner.lock().await;
                let now = Instant::now();

                // Drop entries older than the window.
                while let Some(front) = guard.front() {
                    if now.duration_since(*front) >= window {
                        guard.pop_front();
                    } else {
                        break;
                    }
                }

                if (guard.len() as u32) < self.max_per_minute {
                    guard.push_back(now);
                    self.acquire_count.fetch_add(1, Ordering::Relaxed);
                    return;
                }

                // Compute how long until the oldest entry leaves the window.
                let oldest = *guard.front().expect("len >= max > 0");
                let elapsed = now.duration_since(oldest);
                window.saturating_sub(elapsed) + Duration::from_millis(1)
            }; // mutex guard dropped here

            tokio::time::sleep(sleep_for).await;
        }
    }

    /// Total number of successful `acquire()` calls since construction.
    /// Useful for tests that assert the limiter was actually consulted.
    #[allow(dead_code)]
    pub async fn acquire_count(&self) -> u64 {
        self.acquire_count.load(Ordering::Relaxed)
    }
}
