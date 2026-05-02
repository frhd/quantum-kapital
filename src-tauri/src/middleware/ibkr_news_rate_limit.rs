//! Phase 7 part B — sliding-window rate limiter for the IBKR news
//! `req_historical_news` / `req_news_article` paths.
//!
//! Mirrors [`super::historical_rate_limit::HistoricalRateLimiter`]. The
//! Phase 6 spike paced at one call every 2s; the production v1
//! `IbkrNewsProvider` issues a single `historical_news` call per
//! `fetch(symbol)` (no per-article body fetch — see Phase 7 plan
//! "decisions"), so 30 calls/min is comfortably above the cadence the
//! tracker drives without over-budgeting against TWS pacing.
//!
//! Per-provider serialization is NOT enforced here — the v1 provider
//! batches every subscribed `provider_code` into a single
//! `historical_news` call, so a global window is sufficient. If a
//! future revision splits per-provider calls, swap this for a
//! per-`provider_code` map of windows; the trait stays unchanged.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct IbkrNewsRateLimiter {
    inner: Arc<Mutex<VecDeque<Instant>>>,
    max_per_minute: u32,
    acquire_count: Arc<AtomicU64>,
}

impl IbkrNewsRateLimiter {
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

                let oldest = *guard.front().expect("len >= max > 0");
                let elapsed = now.duration_since(oldest);
                window.saturating_sub(elapsed) + Duration::from_millis(1)
            };

            tokio::time::sleep(sleep_for).await;
        }
    }

    #[allow(dead_code)]
    pub async fn acquire_count(&self) -> u64 {
        self.acquire_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn acquire_within_budget_returns_immediately() {
        let limiter = IbkrNewsRateLimiter::new(30);
        let start = Instant::now();
        for _ in 0..3 {
            limiter.acquire().await;
        }
        // Three back-to-back acquires must NOT block — burst capacity
        // covers the budget. We're well clear of any spurious delay.
        assert!(start.elapsed() < Duration::from_millis(50));
        assert_eq!(limiter.acquire_count().await, 3);
    }

    #[tokio::test]
    async fn over_budget_acquire_blocks() {
        // With max_per_minute = 2, two acquires consume the budget;
        // the third must block until the 60s window slides forward.
        // We don't wait the full 60s — wrapping the third call in a
        // short timeout is enough to prove "blocked", which is the
        // invariant the surveillance pipeline depends on (back-off
        // rather than over-budget burst).
        let limiter = IbkrNewsRateLimiter::new(2);
        limiter.acquire().await;
        limiter.acquire().await;

        let third = tokio::time::timeout(Duration::from_millis(150), limiter.acquire()).await;
        assert!(
            third.is_err(),
            "acquire must block while budget is exhausted"
        );
        assert_eq!(limiter.acquire_count().await, 2);
    }
}
