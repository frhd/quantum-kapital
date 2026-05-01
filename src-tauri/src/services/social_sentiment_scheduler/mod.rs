//! Phase 3 — Social-sentiment scheduler.
//!
//! Wakes on a fixed cadence (default 60 minutes), pulls the active
//! watchlist symbols from `tracker_service`, and asks
//! [`SocialSentimentService::fetch_and_persist`] to refresh every
//! provider for them. Independent of the RTH calendar — Reddit /
//! Stocktwits /Apewisdom move on weekends and outside market hours.
//!
//! Test seam: a `Clock` enum mirrors `eod_scheduler::Clock`. Tests can
//! pin the clock and call [`SocialSentimentScheduler::tick`] directly
//! without waiting for the loop interval.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::ibkr::client::StreamHandle;
use crate::services::social_sentiment::SocialSentimentService;
use crate::services::tracker_service::TrackerService;

/// Loop polling interval. The actual fetch cadence is `min_interval`
/// below; this constant just sets how often we check the wall clock.
const POLL_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug)]
pub enum Clock {
    Real,
    #[allow(dead_code)] // pinned in tests
    Fixed(DateTime<Utc>),
}

impl Clock {
    fn now(&self) -> DateTime<Utc> {
        match self {
            Clock::Real => Utc::now(),
            Clock::Fixed(dt) => *dt,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields read by tests; loop discards
pub struct SentimentSchedulerOutcome {
    pub fetched_at: DateTime<Utc>,
    pub symbols: usize,
    pub samples_persisted: usize,
}

#[derive(Clone)]
pub struct SocialSentimentScheduler {
    service: Arc<SocialSentimentService>,
    tracker: Arc<TrackerService>,
    /// Minimum gap between successful fetches. Defaults to one hour;
    /// `0` means "every poll" (used by tests).
    min_interval: Duration,
    last_run_at: Arc<RwLock<Option<DateTime<Utc>>>>,
    clock: Arc<RwLock<Clock>>,
}

impl SocialSentimentScheduler {
    pub fn new(
        service: Arc<SocialSentimentService>,
        tracker: Arc<TrackerService>,
        min_interval: Duration,
    ) -> Self {
        Self {
            service,
            tracker,
            min_interval,
            last_run_at: Arc::new(RwLock::new(None)),
            clock: Arc::new(RwLock::new(Clock::Real)),
        }
    }

    #[cfg(test)]
    #[allow(dead_code)] // exposed for future scheduler tests with clock pinning
    pub async fn set_clock(&self, clock: Clock) {
        *self.clock.write().await = clock;
    }

    async fn now(&self) -> DateTime<Utc> {
        self.clock.read().await.now()
    }

    /// One scheduling tick. Returns `Some(_)` when the fetch fired,
    /// `None` if we were inside the cooldown window.
    pub async fn tick(&self) -> Result<Option<SentimentSchedulerOutcome>, String> {
        let now = self.now().await;
        if let Some(last) = *self.last_run_at.read().await {
            let elapsed = now
                .signed_duration_since(last)
                .to_std()
                .unwrap_or(Duration::ZERO);
            if elapsed < self.min_interval {
                return Ok(None);
            }
        }

        // Active watchlist = the surveillance "what should I think about"
        // set. Empty list short-circuits cleanly inside the service.
        let rows = self
            .tracker
            .list(None)
            .await
            .map_err(|e| format!("list watchlist: {e}"))?;
        let symbols: Vec<String> = rows.into_iter().map(|t| t.symbol).collect();

        info!("social-sentiment tick: {} symbols", symbols.len());
        let outcome = self
            .service
            .fetch_and_persist(&symbols)
            .await
            .map_err(|e| format!("fetch_and_persist: {e}"))?;

        *self.last_run_at.write().await = Some(now);

        Ok(Some(SentimentSchedulerOutcome {
            fetched_at: now,
            symbols: symbols.len(),
            samples_persisted: outcome.samples_persisted,
        }))
    }

    /// Spawn the polling loop. Returns a [`StreamHandle`] the caller
    /// holds + stops on shutdown — same shape as the EOD / intraday
    /// schedulers. The first tick fires immediately so a freshly
    /// started app gets sentiment data without waiting an hour.
    pub fn spawn(self: Arc<Self>) -> StreamHandle {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);
        let scheduler = Arc::clone(&self);

        let join = tokio::spawn(async move {
            // Fire once on startup so the UI has data without waiting
            // an hour. `tick` short-circuits if `min_interval` was
            // already met, so the cooldown logic still applies on
            // subsequent loops.
            if let Err(e) = scheduler.tick().await {
                warn!("first social-sentiment tick failed: {e}");
            }
            let mut ticker = tokio::time::interval(POLL_INTERVAL);
            ticker.tick().await; // discard the immediate first tick
            loop {
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                ticker.tick().await;
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = scheduler.tick().await {
                    warn!("social-sentiment tick failed: {e}");
                }
            }
            info!("social-sentiment scheduler stopped");
        });

        StreamHandle::new("social-sentiment scheduler", shutdown, join)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::social_sentiment::apewisdom::ApewisdomProvider;
    use crate::services::social_sentiment::provider::MockHttpFetcher;
    use crate::services::social_sentiment::{ArcProvider, SocialSentimentService};
    use crate::storage::Db;
    use tempfile::NamedTempFile;

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    async fn seed_one_ticker(tracker: &TrackerService, symbol: &str) {
        use crate::ibkr::types::tracker::TrackerSource;
        tracker
            .add(symbol, TrackerSource::Manual, None, vec![], None)
            .await
            .expect("add");
    }

    #[tokio::test]
    async fn tick_runs_when_interval_elapsed_and_persists_samples() {
        let (_tmp, db) = make_db();
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        seed_one_ticker(&tracker, "TSLA").await;

        let http = Arc::new(MockHttpFetcher::new());
        http.respond_with(
            "https://test.local/ape",
            r#"{"results":[{"ticker":"TSLA","rank":1,"mentions":50,
                            "sentiment":"Bullish","sentiment_score":60.0}]}"#,
        );
        let provider: ArcProvider =
            Arc::new(ApewisdomProvider::new(http).with_url("https://test.local/ape"));
        let svc = Arc::new(SocialSentimentService::new(Arc::clone(&db), vec![provider]));

        let scheduler = Arc::new(SocialSentimentScheduler::new(
            svc,
            tracker,
            Duration::ZERO, // no cooldown for the test
        ));
        let outcome = scheduler.tick().await.expect("ok").expect("fired");
        assert_eq!(outcome.symbols, 1);
        // One provider * one symbol = 1 row persisted.
        assert_eq!(outcome.samples_persisted, 1);
    }

    #[tokio::test]
    async fn tick_skips_when_inside_cooldown_window() {
        let (_tmp, db) = make_db();
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        seed_one_ticker(&tracker, "TSLA").await;

        let http = Arc::new(MockHttpFetcher::new());
        http.respond_with(
            "https://test.local/ape",
            r#"{"results":[{"ticker":"TSLA","rank":1,"mentions":1,"sentiment":"Neutral","sentiment_score":0.0}]}"#,
        );
        let provider: ArcProvider =
            Arc::new(ApewisdomProvider::new(http).with_url("https://test.local/ape"));
        let svc = Arc::new(SocialSentimentService::new(Arc::clone(&db), vec![provider]));

        let scheduler = Arc::new(SocialSentimentScheduler::new(
            svc,
            tracker,
            Duration::from_secs(3600),
        ));
        let first = scheduler.tick().await.expect("ok");
        assert!(first.is_some(), "first tick fires");
        let second = scheduler.tick().await.expect("ok");
        assert!(
            second.is_none(),
            "second tick within 1h cooldown is a no-op"
        );
    }
}
