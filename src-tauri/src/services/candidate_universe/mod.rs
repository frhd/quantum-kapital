//! Phase 4 — Candidate-universe staging layer.
//!
//! Decouples "scanner found a ticker" from "ticker is in the watchlist".
//! Every scanner profile (broad IBKR scans, sentiment surges, future
//! earnings movers) produces a [`NewCandidate`] which the service
//! upserts into `candidate_universe`. Promotion into the live
//! `tracked_tickers` watchlist is orthogonal: either
//! [`crate::services::candidate_promoter`] auto-promotes high-score
//! rows or the agent calls the `promote_candidate` MCP tool.
//!
//! Test seam: [`CandidateClock`] mirrors the
//! [`crate::services::social_sentiment::ClockFn`] pattern so unit tests
//! pin a deterministic `now` without leaking real wall-clock behaviour.

#![allow(dead_code)] // wired into lib.rs / scheduler / MCP tools in subsequent Phase-4 steps

pub mod repo;
pub mod types;

use std::sync::Arc;

use chrono::Utc;

#[allow(unused_imports)]
pub use types::{Candidate, CandidateFilter, CandidateSource, NewCandidate};

use crate::storage::{Db, Result as StorageResult};

/// Wall-clock seam — production wires [`SystemClock`], tests pin a
/// fixed value via [`FixedClock`].
pub trait CandidateClock: Send + Sync {
    fn now_unix(&self) -> i64;
}

pub struct SystemClock;
impl CandidateClock for SystemClock {
    fn now_unix(&self) -> i64 {
        Utc::now().timestamp()
    }
}

#[cfg(test)]
pub struct FixedClock(pub std::sync::atomic::AtomicI64);
#[cfg(test)]
impl CandidateClock for FixedClock {
    fn now_unix(&self) -> i64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Outcome of a single [`CandidateUniverseService::decay`] run. Logged
/// by the scheduler; tests assert on it.
#[derive(Debug, Clone, Default)]
pub struct DecayOutcome {
    pub evicted: usize,
}

#[derive(Clone)]
pub struct CandidateUniverseService {
    db: Arc<Db>,
    clock: Arc<dyn CandidateClock>,
}

impl CandidateUniverseService {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            clock: Arc::new(SystemClock),
        }
    }

    #[allow(dead_code)] // exercised by tests + the decay scheduler when wired
    pub fn with_clock(mut self, clock: Arc<dyn CandidateClock>) -> Self {
        self.clock = clock;
        self
    }

    /// Insert or merge a candidate produced by any scanner-like source.
    /// The merged row is returned so callers can decide whether to
    /// auto-promote without an extra read.
    pub async fn upsert(&self, new: NewCandidate) -> StorageResult<Candidate> {
        let now = self.clock.now_unix();
        repo::upsert(Arc::clone(&self.db), new, now).await
    }

    /// List candidates matching `filter`. Default behaviour hides
    /// promoted rows so the agent's inbox stays clean.
    pub async fn list(&self, filter: CandidateFilter) -> StorageResult<Vec<Candidate>> {
        repo::list(Arc::clone(&self.db), filter).await
    }

    /// Single-row lookup by symbol. Returns `None` for unknowns.
    #[allow(dead_code)] // consumed by `promote_candidate` MCP tool
    pub async fn get(&self, symbol: &str) -> StorageResult<Option<Candidate>> {
        repo::get(Arc::clone(&self.db), symbol.to_string()).await
    }

    /// Stamp `promoted_at = now`. Idempotent. Returns the post-update
    /// row, or `None` if no candidate exists for `symbol`.
    pub async fn mark_promoted(&self, symbol: &str) -> StorageResult<Option<Candidate>> {
        let now = self.clock.now_unix();
        repo::mark_promoted(Arc::clone(&self.db), symbol.to_string(), now).await
    }

    /// Run the decay sweep — deletes unpromoted rows whose `decay_at`
    /// has passed. Returns the eviction count.
    pub async fn decay(&self) -> StorageResult<DecayOutcome> {
        let now = self.clock.now_unix();
        let evicted = repo::delete_expired(Arc::clone(&self.db), now).await?;
        Ok(DecayOutcome { evicted })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicI64;
    use tempfile::NamedTempFile;

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    fn fixed(now: i64) -> Arc<dyn CandidateClock> {
        Arc::new(FixedClock(AtomicI64::new(now)))
    }

    fn new_candidate(symbol: &str, source_id: &str, score: f64, ttl: i64) -> NewCandidate {
        NewCandidate {
            symbol: symbol.to_string(),
            source: CandidateSource {
                source: source_id.to_string(),
                score,
                rank: None,
                meta: json!({}),
                last_seen: 0,
            },
            reason_md: Some("test reason".into()),
            ttl_seconds: ttl,
        }
    }

    #[tokio::test]
    async fn upsert_uses_injected_clock_for_now() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_321_i64;
        let svc = CandidateUniverseService::new(db).with_clock(fixed(now));

        let saved = svc
            .upsert(new_candidate("TSLA", "scanner_top_perc_gain", 0.6, 86_400))
            .await
            .unwrap();
        assert_eq!(saved.first_seen, now);
        assert_eq!(saved.last_seen, now);
        assert_eq!(saved.decay_at, now + 86_400);
    }

    #[tokio::test]
    async fn mark_promoted_then_list_excludes_by_default() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let svc = CandidateUniverseService::new(db).with_clock(fixed(now));

        svc.upsert(new_candidate("AAA", "scanner_top_perc_gain", 0.5, 86_400))
            .await
            .unwrap();
        svc.upsert(new_candidate("BBB", "scanner_top_perc_gain", 0.6, 86_400))
            .await
            .unwrap();
        let promoted = svc.mark_promoted("aaa").await.unwrap().expect("row");
        assert!(promoted.promoted_at.is_some());

        let pending = svc.list(CandidateFilter::default()).await.unwrap();
        let symbols: Vec<_> = pending.iter().map(|c| c.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["BBB"]);
    }

    #[tokio::test]
    async fn decay_evicts_only_unpromoted_expired_rows() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let fc = Arc::new(FixedClock(AtomicI64::new(now)));
        let svc =
            CandidateUniverseService::new(db).with_clock(Arc::clone(&fc) as Arc<dyn CandidateClock>);

        // Insert at t=now with ttl=60 -> decay at now+60.
        svc.upsert(new_candidate("STALE", "scanner_top_perc_gain", 0.5, 60))
            .await
            .unwrap();
        svc.upsert(new_candidate("FRESH", "scanner_top_perc_gain", 0.5, 86_400))
            .await
            .unwrap();

        // Bump the clock past the stale's deadline.
        fc.0.store(now + 120, std::sync::atomic::Ordering::Relaxed);
        let outcome = svc.decay().await.unwrap();
        assert_eq!(outcome.evicted, 1);

        let remaining = svc.list(CandidateFilter::default()).await.unwrap();
        let symbols: Vec<_> = remaining.iter().map(|c| c.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["FRESH"]);
    }
}
