//! Phase 4 — synthetic candidate source that turns social-sentiment
//! mention spikes into [`crate::services::candidate_universe::Candidate`]s.
//!
//! Joins the `social_sentiment` table (Phase 3) against itself to find
//! tickers whose recent mention volume is meaningfully above their 7-
//! day baseline — independent of the deterministic detector pipeline,
//! so a brand-new ticker the watchlist has never seen can still surface
//! in the morning candidate set.
//!
//! Scoring: `1 - exp(-spike_ratio / 4)`. A 4× spike scores ≈0.63, an
//! 8× spike ≈0.86, a 16× spike ≈0.98. Clamped to `[0, 1]`. The
//! `min_baseline_mentions` floor keeps single-mention tickers from
//! producing infinite ratios.
//!
//! Decay: 48h. Sentiment surges go cold fast; without a refresh tick
//! the candidate falls out of the inbox by the next morning.
//!
//! Test seam: a [`CandidateClock`] is taken so unit tests can pin the
//! `now` used for both the SQL window calculation and the
//! `last_seen` / `decay_at` stamps.

#![allow(dead_code)] // wired in lib.rs + scheduler + smoke tests in subsequent steps

use std::sync::Arc;

use rusqlite::params;
use serde_json::json;
use tracing::{info, warn};

use crate::services::candidate_promoter::CandidatePromoter;
use crate::services::candidate_universe::types::{CandidateSource, NewCandidate};
use crate::services::candidate_universe::{CandidateClock, SystemClock};
use crate::storage::Db;

const SOURCE_ID: &str = "sentiment_surge";
const TTL_SECONDS: i64 = 2 * 86_400;
const RECENT_WINDOW_SECONDS: i64 = 24 * 3_600;
const BASELINE_WINDOW_SECONDS: i64 = 7 * 86_400;
/// Below this 24-hour mention count we don't consider the ticker —
/// keeps "first appearance" rows out of the candidate inbox.
const DEFAULT_MIN_RECENT_MENTIONS: i64 = 30;
/// Floor for the baseline count when computing spike ratio. Without a
/// floor a single mention three days ago becomes a 30× spike.
const BASELINE_FLOOR: f64 = 5.0;
/// Spike ratios at or below 1.0 are noise.
const MIN_SPIKE_RATIO: f64 = 1.5;

/// Outcome of a single [`SentimentSurgeScanner::run_once`] call.
#[derive(Debug, Clone, Default)]
pub struct SurgeRunOutcome {
    /// Symbols upserted into `candidate_universe` this run.
    pub upserted: Vec<String>,
    /// Symbols that crossed the promoter's auto-threshold and landed
    /// on the watchlist.
    pub auto_promoted: Vec<String>,
}

#[derive(Clone)]
pub struct SentimentSurgeScanner {
    db: Arc<Db>,
    promoter: Arc<CandidatePromoter>,
    clock: Arc<dyn CandidateClock>,
    min_recent_mentions: i64,
}

impl SentimentSurgeScanner {
    pub fn new(db: Arc<Db>, promoter: Arc<CandidatePromoter>) -> Self {
        Self {
            db,
            promoter,
            clock: Arc::new(SystemClock),
            min_recent_mentions: DEFAULT_MIN_RECENT_MENTIONS,
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn CandidateClock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_min_recent_mentions(mut self, n: i64) -> Self {
        self.min_recent_mentions = n;
        self
    }

    /// Compute the 24h-vs-7d mention spike per symbol, upsert any
    /// surge into `candidate_universe`, and let the promoter decide
    /// whether to auto-promote. Returns the per-symbol breakdown for
    /// scheduler / test introspection.
    pub async fn run_once(&self) -> Result<SurgeRunOutcome, String> {
        let now = self.clock.now_unix();
        let surges = self
            .compute_surges(now)
            .await
            .map_err(|e| format!("compute_surges: {e}"))?;
        let mut outcome = SurgeRunOutcome::default();
        for surge in surges {
            let candidate = NewCandidate {
                symbol: surge.symbol.clone(),
                source: CandidateSource {
                    source: SOURCE_ID.to_string(),
                    score: surge.score,
                    rank: None,
                    meta: json!({
                        "recent_mentions": surge.recent_mentions,
                        "baseline_mentions": surge.baseline_mentions,
                        "spike_ratio": surge.spike_ratio,
                        "window_seconds": RECENT_WINDOW_SECONDS,
                        "baseline_window_seconds": BASELINE_WINDOW_SECONDS,
                    }),
                    last_seen: 0,
                },
                reason_md: Some(format!(
                    "{} mentions surged {:.1}× ({} → {} in 24h)",
                    surge.symbol,
                    surge.spike_ratio,
                    surge.baseline_mentions,
                    surge.recent_mentions
                )),
                ttl_seconds: TTL_SECONDS,
            };
            let merged = match self.promoter.candidates().upsert(candidate).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("sentiment_surge upsert failed for {}: {e}", surge.symbol);
                    continue;
                }
            };
            outcome.upserted.push(merged.symbol.clone());
            if matches!(
                self.promoter.try_auto_promote(&merged).await,
                crate::services::candidate_promoter::PromotionOutcome::Promoted
            ) {
                outcome.auto_promoted.push(merged.symbol);
            }
        }
        info!(
            "sentiment_surge: {} surged, {} auto-promoted",
            outcome.upserted.len(),
            outcome.auto_promoted.len()
        );
        Ok(outcome)
    }

    async fn compute_surges(&self, now: i64) -> Result<Vec<Surge>, String> {
        let recent_since = now - RECENT_WINDOW_SECONDS;
        let baseline_since = now - BASELINE_WINDOW_SECONDS;
        let min_recent = self.min_recent_mentions;
        self.db
            .with_conn(move |conn| {
                // For each symbol: SUM(mentions_24h) over the last 24h
                // and the prior 7d. We scale the 7d sum by 1/7 to put
                // both numbers on a "mentions per day" basis, then
                // divide. SQLite has no `GREATEST`, hence the explicit
                // MAX(...) inside Rust below.
                let mut stmt = conn.prepare(
                    "SELECT symbol, \
                            COALESCE(SUM(CASE WHEN fetched_at >= ?1 THEN mentions_24h END), 0) AS recent, \
                            COALESCE(SUM(CASE WHEN fetched_at >= ?2 AND fetched_at < ?1 THEN mentions_24h END), 0) AS prior \
                     FROM social_sentiment \
                     WHERE fetched_at >= ?2 AND mentions_24h IS NOT NULL AND is_stale = 0 \
                     GROUP BY symbol",
                )?;
                let rows = stmt
                    .query_map(params![recent_since, baseline_since], |row| {
                        Ok((
                            row.get::<_, String>("symbol")?,
                            row.get::<_, i64>("recent")?,
                            row.get::<_, i64>("prior")?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut surges = Vec::new();
                for (symbol, recent, prior) in rows {
                    if recent < min_recent {
                        continue;
                    }
                    // Scale prior to a per-day average; the recent
                    // bucket is already 24h.
                    let baseline_per_day = (prior as f64) / 6.0; // 7d window minus the 24h recent slice
                    let baseline_clamped = baseline_per_day.max(BASELINE_FLOOR);
                    let spike_ratio = (recent as f64) / baseline_clamped;
                    if spike_ratio < MIN_SPIKE_RATIO {
                        continue;
                    }
                    // 1 - exp(-r/4): saturating curve in [0, 1).
                    let score = 1.0 - (-spike_ratio / 4.0).exp();
                    surges.push(Surge {
                        symbol,
                        recent_mentions: recent,
                        baseline_mentions: prior,
                        spike_ratio,
                        score,
                    });
                }
                // Highest score first; tests + schedulers rely on this.
                surges.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
                Ok(surges)
            })
            .await
            .map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone)]
struct Surge {
    symbol: String,
    recent_mentions: i64,
    baseline_mentions: i64,
    spike_ratio: f64,
    score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::candidate_universe::{
        CandidateUniverseService, FixedClock as CandidateFixedClock,
    };
    use crate::services::tracker_service::TrackerService;
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

    fn build_scanner(
        db: Arc<Db>,
        clock_now: i64,
        threshold: f64,
    ) -> (
        SentimentSurgeScanner,
        Arc<CandidateUniverseService>,
        Arc<TrackerService>,
    ) {
        let candidates = Arc::new(
            CandidateUniverseService::new(Arc::clone(&db))
                .with_clock(Arc::new(CandidateFixedClock(AtomicI64::new(clock_now)))),
        );
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        let promoter = Arc::new(CandidatePromoter::new(
            Arc::clone(&candidates),
            Arc::clone(&tracker),
            threshold,
        ));
        let scanner = SentimentSurgeScanner::new(Arc::clone(&db), promoter)
            .with_clock(Arc::new(CandidateFixedClock(AtomicI64::new(clock_now))))
            .with_min_recent_mentions(20);
        (scanner, candidates, tracker)
    }

    #[tokio::test]
    async fn run_once_emits_candidate_for_high_spike_symbols() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        // SURGE: 6 baseline mentions distributed across the 7d window
        // (one per day, ~5 days back) → baseline_per_day ≈ 1, clamped
        // to BASELINE_FLOOR=5; recent 200 mentions in last 24h → 40×
        // ratio → score ≈ 1.
        let day = 86_400_i64;
        for i in 1..=6 {
            insert_mention(&db, "SURGE", 1, now - day * (1 + i)).await;
        }
        insert_mention(&db, "SURGE", 200, now - 3_600).await;

        // QUIET: small recent volume → filtered by min_recent_mentions.
        insert_mention(&db, "QUIET", 5, now - 3_600).await;

        let (scanner, candidates, _tracker) = build_scanner(db, now, 1.5);
        let outcome = scanner.run_once().await.unwrap();
        assert_eq!(outcome.upserted, vec!["SURGE"]);
        // No promotion because threshold > 1.0 is unreachable.
        assert!(outcome.auto_promoted.is_empty());

        let cand = candidates.get("SURGE").await.unwrap().expect("row");
        assert!(cand.score > 0.9, "score should be near 1; got {}", cand.score);
        assert_eq!(cand.sources.len(), 1);
        assert_eq!(cand.sources[0].source, "sentiment_surge");
        let meta = &cand.sources[0].meta;
        assert_eq!(meta["recent_mentions"], 200);
    }

    #[tokio::test]
    async fn run_once_skips_when_baseline_is_normal() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        // 100 mentions/day baseline, 110 today → ratio < MIN_SPIKE_RATIO.
        let day = 86_400_i64;
        for i in 1..=6 {
            insert_mention(&db, "STEADY", 100, now - day * (1 + i)).await;
        }
        insert_mention(&db, "STEADY", 110, now - 3_600).await;

        let (scanner, candidates, _tracker) = build_scanner(db, now, 0.7);
        let outcome = scanner.run_once().await.unwrap();
        assert!(outcome.upserted.is_empty());
        assert!(candidates.get("STEADY").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn run_once_auto_promotes_above_threshold_and_stamps_promoted_at() {
        let (_tmp, db) = make_db();
        let now = 1_700_000_000_i64;
        let day = 86_400_i64;
        for i in 1..=6 {
            insert_mention(&db, "HOT", 1, now - day * (1 + i)).await;
        }
        insert_mention(&db, "HOT", 500, now - 3_600).await;

        let (scanner, candidates, tracker) = build_scanner(db, now, 0.7);
        let outcome = scanner.run_once().await.unwrap();
        assert_eq!(outcome.auto_promoted, vec!["HOT"]);

        let cand = candidates.get("HOT").await.unwrap().unwrap();
        assert!(cand.promoted_at.is_some());
        let row = tracker.get("HOT").await.unwrap().unwrap();
        assert_eq!(row.source.as_str(), "auto_scanner");
    }
}
