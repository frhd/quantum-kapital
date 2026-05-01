#![allow(dead_code)] // wired into lib.rs once Phase-4 Tauri commands land

//! Phase 4 — combined cadence loop for candidate-universe upkeep.
//!
//! Wakes once per cadence and:
//!
//! 1. Refreshes the synthetic
//!    [`crate::services::sentiment_surge_scanner::SentimentSurgeScanner`]
//!    so social-sentiment spikes flow into `candidate_universe`.
//! 2. Sweeps expired candidates via
//!    [`crate::services::candidate_universe::CandidateUniverseService::decay`].
//!
//! IBKR scanner profiles still run from `AutoScannerScheduler` — that's
//! a separate cadence pegged to RTH. This scheduler is calendar-
//! agnostic: sentiment moves on weekends, and the decay job needs to
//! run regardless of market hours so an "agent inbox" check after a
//! weekend doesn't drown in stale rows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::ibkr::client::StreamHandle;
use crate::services::candidate_universe::CandidateUniverseService;
use crate::services::sentiment_surge_scanner::SentimentSurgeScanner;

/// Loop polling interval. The actual run cadence is controlled by
/// `min_interval` below; this just bounds how often we wake to check.
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

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // fields read by tests; loop discards
pub struct CandidateTickOutcome {
    pub ran_at: Option<DateTime<Utc>>,
    pub surge_upserted: usize,
    pub surge_auto_promoted: usize,
    pub decay_evicted: usize,
}

#[derive(Clone)]
pub struct CandidateScheduler {
    surge: Arc<SentimentSurgeScanner>,
    universe: Arc<CandidateUniverseService>,
    /// Minimum gap between successful runs. `Duration::ZERO` ⇒ every
    /// poll (used by tests).
    min_interval: Duration,
    last_run_at: Arc<RwLock<Option<DateTime<Utc>>>>,
    clock: Arc<RwLock<Clock>>,
}

impl CandidateScheduler {
    pub fn new(
        surge: Arc<SentimentSurgeScanner>,
        universe: Arc<CandidateUniverseService>,
        min_interval: Duration,
    ) -> Self {
        Self {
            surge,
            universe,
            min_interval,
            last_run_at: Arc::new(RwLock::new(None)),
            clock: Arc::new(RwLock::new(Clock::Real)),
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn set_clock(&self, clock: Clock) {
        *self.clock.write().await = clock;
    }

    async fn now(&self) -> DateTime<Utc> {
        self.clock.read().await.now()
    }

    /// Run a single tick. Returns `Some(_)` when the work fired,
    /// `None` when we were inside the cooldown window.
    pub async fn tick(&self) -> Result<Option<CandidateTickOutcome>, String> {
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

        let surge_outcome = self
            .surge
            .run_once()
            .await
            .map_err(|e| format!("sentiment_surge: {e}"))?;
        let decay_outcome = self
            .universe
            .decay()
            .await
            .map_err(|e| format!("decay: {e}"))?;

        *self.last_run_at.write().await = Some(now);
        info!(
            "candidate-scheduler tick: surge_upserted={} surge_promoted={} decay_evicted={}",
            surge_outcome.upserted.len(),
            surge_outcome.auto_promoted.len(),
            decay_outcome.evicted
        );
        Ok(Some(CandidateTickOutcome {
            ran_at: Some(now),
            surge_upserted: surge_outcome.upserted.len(),
            surge_auto_promoted: surge_outcome.auto_promoted.len(),
            decay_evicted: decay_outcome.evicted,
        }))
    }

    /// Spawn the polling loop. Same `StreamHandle` shape as the other
    /// schedulers; the first tick fires immediately so the candidate
    /// inbox isn't empty for an hour after startup.
    #[allow(dead_code)] // wired in lib.rs alongside the other schedulers
    pub fn spawn(self: Arc<Self>) -> StreamHandle {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);
        let scheduler = Arc::clone(&self);

        let join = tokio::spawn(async move {
            if let Err(e) = scheduler.tick().await {
                warn!("first candidate-scheduler tick failed: {e}");
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
                    warn!("candidate-scheduler tick failed: {e}");
                }
            }
            info!("candidate-scheduler stopped");
        });

        StreamHandle::new("candidate scheduler", shutdown, join)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::candidate_promoter::CandidatePromoter;
    use crate::services::candidate_universe::{CandidateUniverseService, FixedClock as CClock};
    use crate::services::tracker_service::TrackerService;
    use crate::storage::Db;
    use rusqlite::params;
    use std::sync::atomic::AtomicI64;
    use tempfile::NamedTempFile;

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    async fn insert_mention(db: &Arc<Db>, symbol: &str, mentions: i64, fetched_at: i64) {
        let symbol = symbol.to_string();
        db.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO social_sentiment \
                 (source, symbol, score, mentions_24h, sentiment_label, rank, raw_payload, is_stale, fetched_at) \
                 VALUES ('apewisdom', ?1, NULL, ?2, NULL, NULL, '{}', 0, ?3)",
                params![symbol, mentions, fetched_at],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    }

    fn build_scheduler(db: Arc<Db>, clock_now: i64, min_interval: Duration) -> CandidateScheduler {
        let candidates = Arc::new(
            CandidateUniverseService::new(Arc::clone(&db))
                .with_clock(Arc::new(CClock(AtomicI64::new(clock_now)))),
        );
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        let promoter = Arc::new(CandidatePromoter::new(
            Arc::clone(&candidates),
            Arc::clone(&tracker),
            0.7,
        ));
        let surge = Arc::new(
            SentimentSurgeScanner::new(Arc::clone(&db), promoter)
                .with_clock(Arc::new(CClock(AtomicI64::new(clock_now))))
                .with_min_recent_mentions(20),
        );
        CandidateScheduler::new(surge, candidates, min_interval)
    }

    #[tokio::test]
    async fn tick_runs_surge_and_decay_together() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        // Seed a surge candidate.
        let day = 86_400_i64;
        for i in 1..=6 {
            insert_mention(&db, "HOT", 1, now - day * (1 + i)).await;
        }
        insert_mention(&db, "HOT", 200, now - 3_600).await;

        let scheduler = build_scheduler(Arc::clone(&db), now, Duration::ZERO);
        let outcome = scheduler.tick().await.unwrap().expect("fired");
        assert_eq!(outcome.surge_upserted, 1);
        // No expired rows yet, so decay evicts nothing.
        assert_eq!(outcome.decay_evicted, 0);
    }

    #[tokio::test]
    async fn tick_skips_inside_cooldown() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let scheduler = build_scheduler(db, now, Duration::from_secs(3600));
        let first = scheduler.tick().await.unwrap();
        assert!(first.is_some());
        let second = scheduler.tick().await.unwrap();
        assert!(second.is_none(), "cooldown blocks second tick");
    }
}
