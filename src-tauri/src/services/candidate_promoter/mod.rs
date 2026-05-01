//! Phase 4 — promotion logic between `candidate_universe` (staging)
//! and `tracked_tickers` (live watchlist).
//!
//! Two paths land in `tracked_tickers`:
//!
//! 1. **Auto-promotion** — every scanner upsert produces a fresh
//!    `Candidate`; if the merged score is `>= auto_threshold` and the
//!    symbol isn't already on the watchlist, [`try_auto_promote`] adds
//!    it with `source = AutoScanner` and stamps `promoted_at` on the
//!    candidate row.
//! 2. **Agent / interactive promotion** — the `promote_candidate` MCP
//!    tool calls [`promote_for_agent`] which adds with `source = Agent`
//!    and the agent's reasoning as the watchlist row's notes.
//!
//! Both paths share [`CandidatePromoter`] so the audit trail (mark the
//! `candidate_universe.promoted_at`) is consistent regardless of who
//! initiated the promotion.

#![allow(dead_code)] // wired in lib.rs / auto_scanner / MCP tools in subsequent steps

use std::sync::Arc;

use serde_json::json;
use tracing::{info, warn};

use crate::ibkr::types::tracker::TrackerSource;
use crate::services::candidate_universe::types::Candidate;
use crate::services::candidate_universe::CandidateUniverseService;
use crate::services::tracker_service::{TrackerError, TrackerService};

/// Outcome of a promotion attempt. Returned for tests + the auto-scanner
/// loop's structured logging.
#[derive(Debug, Clone, PartialEq)]
pub enum PromotionOutcome {
    /// The watchlist row was created and `candidate_universe.promoted_at`
    /// was stamped.
    Promoted,
    /// Already on the watchlist (or was). Idempotent — `promoted_at`
    /// is still stamped if the candidate row exists.
    AlreadyTracked,
    /// Auto-promote path only: score below the configured threshold,
    /// no watchlist write performed.
    BelowThreshold { score: f64, threshold: f64 },
    /// Auto-promote path only: candidate is currently in cooldown
    /// (re-promote suppression window after a prior auto-promote).
    InCooldown { until: i64 },
}

#[derive(Clone)]
pub struct CandidatePromoter {
    candidates: Arc<CandidateUniverseService>,
    tracker: Arc<TrackerService>,
    auto_threshold: f64,
}

impl CandidatePromoter {
    pub fn new(
        candidates: Arc<CandidateUniverseService>,
        tracker: Arc<TrackerService>,
        auto_threshold: f64,
    ) -> Self {
        Self {
            candidates,
            tracker,
            auto_threshold,
        }
    }

    pub fn auto_threshold(&self) -> f64 {
        self.auto_threshold
    }

    /// Read-only handle for callers that need to upsert candidates
    /// before invoking [`Self::try_auto_promote`] (the auto-scanner
    /// loop, sentiment-surge scanner, etc.).
    pub fn candidates(&self) -> &Arc<CandidateUniverseService> {
        &self.candidates
    }

    /// Score-gated promotion called by the scanner pipelines.
    ///
    /// Skips silently when the candidate is already promoted (idempotent
    /// — re-running the scanner doesn't churn the watchlist) or scores
    /// below `auto_threshold`. The `meta` payload stamped on the
    /// watchlist row carries the candidate's source provenance so the
    /// UI's "added by" column can attribute the row.
    pub async fn try_auto_promote(&self, candidate: &Candidate) -> PromotionOutcome {
        if candidate.promoted_at.is_some() {
            return PromotionOutcome::AlreadyTracked;
        }
        if candidate.score < self.auto_threshold {
            return PromotionOutcome::BelowThreshold {
                score: candidate.score,
                threshold: self.auto_threshold,
            };
        }
        let meta = json!({
            "via": "candidate_universe",
            "score": candidate.score,
            "sources": candidate.sources,
        });
        let outcome = self
            .tracker
            .add(
                &candidate.symbol,
                TrackerSource::AutoScanner,
                Some(meta),
                vec![],
                candidate.reason_md.clone(),
            )
            .await;
        match outcome {
            Ok(_) => {
                self.stamp_promoted(&candidate.symbol).await;
                info!(
                    "candidate_promoter: auto-promoted {} (score {:.2} >= {:.2})",
                    candidate.symbol, candidate.score, self.auto_threshold
                );
                PromotionOutcome::Promoted
            }
            Err(TrackerError::AlreadyTracked(_)) => {
                self.stamp_promoted(&candidate.symbol).await;
                PromotionOutcome::AlreadyTracked
            }
            Err(e) => {
                warn!(
                    "candidate_promoter: auto-promote failed for {}: {e}",
                    candidate.symbol
                );
                PromotionOutcome::BelowThreshold {
                    score: candidate.score,
                    threshold: self.auto_threshold,
                }
            }
        }
    }

    /// Agent / interactive promotion. Bypasses the score threshold —
    /// the caller has explicit reasoning. Returns `Ok(PromotionOutcome)`
    /// even when the symbol was already tracked; the candidate row is
    /// stamped either way so the staging "inbox" no longer surfaces it.
    pub async fn promote_for_agent(
        &self,
        symbol: &str,
        reason: &str,
    ) -> Result<PromotionOutcome, String> {
        let symbol_upper = symbol.to_uppercase();
        // Pull the candidate so the watchlist row can carry provenance.
        let candidate = self
            .candidates
            .get(&symbol_upper)
            .await
            .map_err(|e| format!("load candidate: {e}"))?;
        let meta = candidate.as_ref().map(|c| {
            json!({
                "via": "candidate_universe",
                "score": c.score,
                "sources": c.sources,
            })
        });
        let outcome = self
            .tracker
            .add(
                &symbol_upper,
                TrackerSource::Agent,
                meta,
                vec![],
                Some(reason.to_string()),
            )
            .await;
        match outcome {
            Ok(_) => {
                if candidate.is_some() {
                    self.stamp_promoted(&symbol_upper).await;
                }
                info!("candidate_promoter: agent promoted {symbol_upper} (reason='{reason}')");
                Ok(PromotionOutcome::Promoted)
            }
            Err(TrackerError::AlreadyTracked(_)) => {
                if candidate.is_some() {
                    self.stamp_promoted(&symbol_upper).await;
                }
                Ok(PromotionOutcome::AlreadyTracked)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    async fn stamp_promoted(&self, symbol: &str) {
        if let Err(e) = self.candidates.mark_promoted(symbol).await {
            warn!("candidate_promoter: mark_promoted({symbol}) failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::candidate_universe::types::{CandidateSource, NewCandidate};
    use crate::services::candidate_universe::{CandidateUniverseService, FixedClock};
    use crate::storage::Db;
    use std::sync::atomic::AtomicI64;
    use tempfile::NamedTempFile;

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    fn make_promoter(
        db: Arc<Db>,
        threshold: f64,
        clock_now: i64,
    ) -> (
        CandidatePromoter,
        Arc<TrackerService>,
        Arc<CandidateUniverseService>,
    ) {
        let candidates = Arc::new(
            CandidateUniverseService::new(Arc::clone(&db))
                .with_clock(Arc::new(FixedClock(AtomicI64::new(clock_now)))),
        );
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        let promoter =
            CandidatePromoter::new(Arc::clone(&candidates), Arc::clone(&tracker), threshold);
        (promoter, tracker, candidates)
    }

    async fn upsert_candidate(
        candidates: &CandidateUniverseService,
        symbol: &str,
        score: f64,
    ) -> Candidate {
        let new = NewCandidate {
            symbol: symbol.to_string(),
            source: CandidateSource {
                source: "scanner_top_perc_gain".into(),
                score,
                rank: Some(1),
                meta: serde_json::json!({}),
                last_seen: 0,
            },
            reason_md: Some(format!("top gainer: {symbol}")),
            ttl_seconds: 7 * 86_400,
        };
        candidates.upsert(new).await.unwrap()
    }

    #[tokio::test]
    async fn try_auto_promote_skips_below_threshold() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let (promoter, tracker, candidates) = make_promoter(db, 0.7, now);

        let cand = upsert_candidate(&candidates, "AAA", 0.4).await;
        let outcome = promoter.try_auto_promote(&cand).await;
        match outcome {
            PromotionOutcome::BelowThreshold { score, threshold } => {
                assert_eq!(score, 0.4);
                assert_eq!(threshold, 0.7);
            }
            other => panic!("expected BelowThreshold, got {other:?}"),
        }
        // Watchlist untouched, candidate still un-promoted.
        assert!(tracker.list(None).await.unwrap().is_empty());
        let still = candidates.get("AAA").await.unwrap().unwrap();
        assert!(still.promoted_at.is_none());
    }

    #[tokio::test]
    async fn try_auto_promote_adds_to_watchlist_when_above_threshold() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let (promoter, tracker, candidates) = make_promoter(db, 0.7, now);

        let cand = upsert_candidate(&candidates, "AAA", 0.85).await;
        let outcome = promoter.try_auto_promote(&cand).await;
        assert_eq!(outcome, PromotionOutcome::Promoted);

        let watchlist = tracker.list(None).await.unwrap();
        assert_eq!(watchlist.len(), 1);
        assert_eq!(watchlist[0].symbol, "AAA");
        assert_eq!(watchlist[0].source.as_str(), "auto_scanner");
        // source_meta carries the candidate provenance.
        let meta = watchlist[0].source_meta.as_ref().unwrap();
        assert_eq!(meta["via"], "candidate_universe");
        assert!(meta["sources"].is_array());

        // Candidate row has promoted_at stamped.
        let after = candidates.get("AAA").await.unwrap().unwrap();
        assert_eq!(after.promoted_at, Some(now));
    }

    #[tokio::test]
    async fn try_auto_promote_idempotent_when_already_tracked() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let (promoter, tracker, candidates) = make_promoter(db, 0.7, now);

        // Pre-add via tracker directly (simulates a manual add the
        // scanner doesn't know about).
        tracker
            .add("BBB", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        let cand = upsert_candidate(&candidates, "BBB", 0.9).await;
        let outcome = promoter.try_auto_promote(&cand).await;
        assert_eq!(outcome, PromotionOutcome::AlreadyTracked);
        // Candidate still got stamped so the agent inbox doesn't keep
        // surfacing it.
        let after = candidates.get("BBB").await.unwrap().unwrap();
        assert_eq!(after.promoted_at, Some(now));
    }

    #[tokio::test]
    async fn try_auto_promote_skips_when_candidate_already_promoted() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let (promoter, _tracker, candidates) = make_promoter(db, 0.7, now);

        let mut cand = upsert_candidate(&candidates, "CCC", 0.95).await;
        candidates.mark_promoted("CCC").await.unwrap();
        cand.promoted_at = Some(now);
        let outcome = promoter.try_auto_promote(&cand).await;
        assert_eq!(outcome, PromotionOutcome::AlreadyTracked);
    }

    #[tokio::test]
    async fn promote_for_agent_uses_agent_source_and_stamps_candidate() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let (promoter, tracker, candidates) = make_promoter(db, 0.95, now);

        // Low-score candidate the agent promotes anyway.
        upsert_candidate(&candidates, "DDD", 0.3).await;
        let outcome = promoter
            .promote_for_agent("ddd", "i like it")
            .await
            .unwrap();
        assert_eq!(outcome, PromotionOutcome::Promoted);

        let row = tracker.get("DDD").await.unwrap().unwrap();
        assert_eq!(row.source.as_str(), "agent");
        assert_eq!(row.notes.as_deref(), Some("i like it"));

        let after = candidates.get("DDD").await.unwrap().unwrap();
        assert!(after.promoted_at.is_some());
    }

    #[tokio::test]
    async fn promote_for_agent_works_for_unknown_candidate() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let (promoter, tracker, candidates) = make_promoter(db, 0.5, now);

        // No candidate row at all — agent is bypassing staging entirely.
        let outcome = promoter.promote_for_agent("EEE", "from gut").await.unwrap();
        assert_eq!(outcome, PromotionOutcome::Promoted);
        assert_eq!(
            tracker.get("EEE").await.unwrap().unwrap().source.as_str(),
            "agent"
        );
        // No candidate row means nothing to stamp — `mark_promoted`
        // returns None silently.
        assert!(candidates.get("EEE").await.unwrap().is_none());
    }
}
