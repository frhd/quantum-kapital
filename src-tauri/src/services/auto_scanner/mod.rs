//! Phase-1 automation: scheduled IBKR scanner runs feed the
//! `candidate_universe` staging layer; from there
//! [`crate::services::candidate_promoter::CandidatePromoter`] either
//! auto-promotes top-scoring rows into the tracker watchlist or leaves
//! them for the agent to review via `promote_candidate`.
//!
//! Phase 4 inverted the previous direct-add behaviour: every scanner
//! row now lands as a [`crate::services::candidate_universe::Candidate`]
//! first; the promoter is the only writer touching `tracked_tickers`.
//! Score-by-rank is `1.0 - (rank - 1) * 0.05`, floored at 0; the
//! promoter's `auto_threshold` plus the per-profile `promote_top_k`
//! still cap how many rows leave staging per run.
//!
//! See `services/auto_scanner/tests.rs` for the behavioural contract.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::params;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::ibkr::client::StreamHandle;
use crate::utils::market_calendar::is_rth_open;

use crate::config::settings::{AutoScannerConfig, ScanProfile};
use crate::ibkr::client::IbkrClient;
use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::tracker::TrackerSource;
use crate::ibkr::types::{ScannerData, ScannerSubscription};
use crate::services::candidate_promoter::{CandidatePromoter, PromotionOutcome};
use crate::services::candidate_universe::types::{CandidateSource, NewCandidate};
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;

/// Default decay window for IBKR scanner candidates. Matches the
/// "7 days for fundamentals-driven" half of the phase plan; sentiment
/// surges (handled by their own scanner) decay faster.
const SCANNER_TTL_SECONDS: i64 = 7 * 86_400;

/// Score-by-rank curve for IBKR scanner rows. Top-of-list = 1.0,
/// decays 0.05 per rank, floored at 0. Pairs with the promoter's
/// `auto_threshold` (default 0.7 ⇒ top 7 rows auto-promote, the rest
/// stage for agent review).
fn rank_score(rank: i32) -> f64 {
    let r = rank.max(1) as f64;
    (1.0 - (r - 1.0) * 0.05).max(0.0)
}

/// Source identifier persisted in `candidate_universe.sources[].source`.
/// `scanner_<lowered scan_code>` (with `::<industry>` suffix when the
/// profile is industry-filtered) so multi-source dedup at upsert time
/// merges hits from the same scan but keeps independent profiles
/// distinct.
fn source_id_for(profile: &ScanProfile) -> String {
    let base = format!("scanner_{}", profile.scan_code.to_lowercase());
    match &profile.industry_filter {
        Some(ind) => format!(
            "{base}::{}",
            ind.to_lowercase().replace(char::is_whitespace, "_")
        ),
        None => base,
    }
}

#[cfg(test)]
mod tests;

/// Narrow trait covering only the one-shot scan needed by this service.
/// Mirrors the `HistoricalDataFetcher` pattern in
/// `services::historical_data_service` — production wires it to
/// [`IbkrClient`], tests use a hand-rolled fake or the existing
/// [`MockIbkrClient`].
#[async_trait]
pub trait MarketScanner: Send + Sync {
    async fn scan(&self, subscription: ScannerSubscription) -> IbkrResult<Vec<ScannerData>>;
}

#[async_trait]
impl MarketScanner for IbkrClient {
    async fn scan(&self, subscription: ScannerSubscription) -> IbkrResult<Vec<ScannerData>> {
        // 5-second wait is comfortably above IBKR's typical first-batch
        // latency; if it expires we treat it as "no rows this tick".
        self.scan_one_shot(subscription, std::time::Duration::from_secs(5))
            .await
    }
}

#[cfg(test)]
#[async_trait]
impl MarketScanner for crate::ibkr::mocks::MockIbkrClient {
    async fn scan(&self, subscription: ScannerSubscription) -> IbkrResult<Vec<ScannerData>> {
        crate::ibkr::mocks::IbkrClientTrait::scan_market(self, subscription).await
    }
}

/// Outcome of a single [`AutoScannerService::run_once`] call. Returned
/// for observability + tests; the spawned scheduler discards it after
/// logging.
#[derive(Debug, Default, Clone)]
#[allow(dead_code)] // fields read by tests + future UI status surface
pub struct RunSummary {
    /// Symbols actually inserted into `tracked_tickers` this run, in
    /// promotion order.
    pub added: Vec<String>,
    /// Human-readable reasons rows were dropped (dedup, cap, etc.).
    /// Logged at warn-level too; surfaced here for assertions.
    pub skipped: Vec<String>,
    /// Per-profile error strings (e.g. IBKR disconnect). Errors don't
    /// abort the run — other profiles still get a chance.
    pub errors: Vec<String>,
}

#[derive(Clone)]
pub struct AutoScannerService {
    scanner: Arc<dyn MarketScanner>,
    tracker: Arc<TrackerService>,
    promoter: Arc<CandidatePromoter>,
    db: Arc<Db>,
    config: Arc<RwLock<AutoScannerConfig>>,
}

impl AutoScannerService {
    pub fn new(
        scanner: Arc<dyn MarketScanner>,
        tracker: Arc<TrackerService>,
        promoter: Arc<CandidatePromoter>,
        db: Arc<Db>,
        config: AutoScannerConfig,
    ) -> Self {
        Self {
            scanner,
            tracker,
            promoter,
            db,
            config: Arc::new(RwLock::new(config)),
        }
    }

    #[allow(dead_code)] // exposed for the Tauri command surface added later
    pub async fn config(&self) -> AutoScannerConfig {
        self.config.read().await.clone()
    }

    #[allow(dead_code)]
    pub async fn set_config(&self, config: AutoScannerConfig) {
        *self.config.write().await = config;
    }

    /// Execute one scan-and-promote pass. Caller supplies `now` so
    /// scheduler cadence and "today" semantics stay deterministic in
    /// tests; production wires `Utc::now()`.
    pub async fn run_once(&self, now: DateTime<Utc>) -> Result<RunSummary, String> {
        let cfg = self.config.read().await.clone();
        let mut summary = RunSummary::default();
        if !cfg.enabled {
            return Ok(summary);
        }

        let start_of_today = start_of_utc_day(now);
        let already_added_today = self
            .count_auto_adds_since(start_of_today)
            .await
            .map_err(|e| format!("count_auto_adds_since failed: {e}"))?;
        if already_added_today >= cfg.daily_cap {
            warn!(
                "auto-scanner daily cap reached ({} >= {}); skipping run",
                already_added_today, cfg.daily_cap
            );
            summary
                .skipped
                .push(format!("daily cap {} already reached", cfg.daily_cap));
            return Ok(summary);
        }
        let mut remaining_cap = cfg.daily_cap.saturating_sub(already_added_today);

        // Snapshot the current watchlist symbols once so per-profile
        // dedup is O(1). We add to this set as we promote within a
        // single run.
        let mut watched: std::collections::HashSet<String> = self
            .tracker
            .list(None)
            .await
            .map_err(|e| format!("tracker.list failed: {e}"))?
            .into_iter()
            .map(|r| r.symbol)
            .collect();

        for profile in cfg.effective_profiles() {
            if remaining_cap == 0 {
                summary
                    .skipped
                    .push(format!("daily cap exhausted before {}", profile.name));
                break;
            }
            let subscription = subscription_for(&profile);
            let rows = match self.scanner.scan(subscription).await {
                Ok(rows) => rows,
                Err(e) => {
                    warn!("auto-scanner profile '{}' failed: {e}", profile.name);
                    summary.errors.push(format!("{}: {e}", profile.name));
                    continue;
                }
            };
            let mut promoted_this_profile: u32 = 0;
            let source_id = source_id_for(&profile);
            for row in rows {
                if remaining_cap == 0 || promoted_this_profile as usize >= profile.promote_top_k {
                    break;
                }
                let symbol = row.contract.symbol.to_uppercase();
                if symbol.is_empty() {
                    continue;
                }
                if watched.contains(&symbol) {
                    summary
                        .skipped
                        .push(format!("{symbol} already on watchlist"));
                    continue;
                }
                let score = rank_score(row.rank);
                let new_candidate = NewCandidate {
                    symbol: symbol.clone(),
                    source: CandidateSource {
                        source: source_id.clone(),
                        score,
                        rank: Some(i64::from(row.rank)),
                        meta: json!({
                            "profile": profile.name,
                            "scan_code": profile.scan_code,
                            "industry": profile.industry_filter,
                            "leg": row.leg,
                        }),
                        last_seen: 0,
                    },
                    reason_md: Some(format!(
                        "{} via {} (rank {})",
                        symbol, profile.name, row.rank
                    )),
                    ttl_seconds: SCANNER_TTL_SECONDS,
                };
                let candidate = match self.promoter.candidates().upsert(new_candidate).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("auto-scanner candidate upsert failed for {symbol}: {e}");
                        summary.skipped.push(format!("{symbol}: {e}"));
                        continue;
                    }
                };
                match self.promoter.try_auto_promote(&candidate).await {
                    PromotionOutcome::Promoted => {
                        info!(
                            "auto-scanner promoted {symbol} via '{}' (rank {}, score {:.2})",
                            profile.name, row.rank, candidate.score
                        );
                        summary.added.push(symbol.clone());
                        watched.insert(symbol);
                        remaining_cap = remaining_cap.saturating_sub(1);
                        promoted_this_profile += 1;
                    }
                    PromotionOutcome::AlreadyTracked => {
                        // Race: a concurrent path added it. Don't count
                        // toward the cap (we didn't author it) and don't
                        // surface it as a hard skip — the staging row
                        // got stamped so the agent inbox stays clean.
                        watched.insert(symbol);
                    }
                    PromotionOutcome::BelowThreshold { score, threshold } => {
                        summary.skipped.push(format!(
                            "{symbol} staged: score {score:.2} < threshold {threshold:.2}"
                        ));
                    }
                    PromotionOutcome::InCooldown { until } => {
                        summary
                            .skipped
                            .push(format!("{symbol} in cooldown until {until}"));
                    }
                }
            }
        }

        info!(
            "auto-scanner run: added {}, skipped {}, errors {}",
            summary.added.len(),
            summary.skipped.len(),
            summary.errors.len()
        );
        Ok(summary)
    }

    async fn count_auto_adds_since(&self, since: DateTime<Utc>) -> Result<u32, String> {
        let since_unix = since.timestamp();
        let source = TrackerSource::AutoScanner.as_str().to_string();
        self.db
            .with_conn(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM tracked_tickers WHERE source = ?1 AND added_at >= ?2",
                    params![source, since_unix],
                    |r| r.get(0),
                )?;
                Ok(count.max(0) as u32)
            })
            .await
            .map_err(|e| format!("db error: {e}"))
    }
}

pub(crate) fn subscription_for(profile: &ScanProfile) -> ScannerSubscription {
    ScannerSubscription {
        number_of_rows: profile.number_of_rows,
        instrument: "STK".to_string(),
        location_code: profile.location_code.clone(),
        scan_code: profile.scan_code.clone(),
        above_price: profile.above_price,
        below_price: None,
        above_volume: profile.above_volume,
        market_cap_above: None,
        market_cap_below: None,
        industry_filter: profile.industry_filter.clone(),
    }
}

fn start_of_utc_day(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.from_utc_datetime(&now.date_naive().and_hms_opt(0, 0, 0).unwrap())
}

// ---------------- scheduler ----------------

/// Wall-clock seam mirroring [`crate::services::intraday_scheduler::Clock`].
/// Production wires `Real` (`Utc::now()`); tests pin a fixed instant so
/// RTH gating + cadence math is deterministic.
#[derive(Clone, Debug)]
pub enum Clock {
    Real,
    #[allow(dead_code)] // test seam
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

/// Long-running scheduler that wakes every `poll_interval` (≤ 60s) and
/// invokes [`AutoScannerService::run_once`] when (a) we're inside US
/// equity RTH and (b) at least `interval_minutes` from the config has
/// elapsed since the last successful run.
#[derive(Clone)]
pub struct AutoScannerScheduler {
    service: Arc<AutoScannerService>,
    poll_interval: Duration,
    clock: Arc<RwLock<Clock>>,
    last_run_at: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl AutoScannerScheduler {
    pub fn new(service: Arc<AutoScannerService>, poll_interval: Duration) -> Self {
        Self {
            service,
            poll_interval,
            clock: Arc::new(RwLock::new(Clock::Real)),
            last_run_at: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub fn with_clock(
        service: Arc<AutoScannerService>,
        poll_interval: Duration,
        clock: Clock,
    ) -> Self {
        Self {
            service,
            poll_interval,
            clock: Arc::new(RwLock::new(clock)),
            last_run_at: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub async fn set_clock(&self, clock: Clock) {
        *self.clock.write().await = clock;
    }

    /// Run one scheduling tick. Returns `Ok(Some(_))` when
    /// [`AutoScannerService::run_once`] actually fired, `Ok(None)` for
    /// no-op ticks (closed market, inside cadence window, or auto-
    /// scanner disabled in config).
    pub async fn tick(&self) -> Result<Option<RunSummary>, String> {
        let now = self.clock.read().await.now();
        if !is_rth_open(now) {
            return Ok(None);
        }

        let cfg = self.service.config().await;
        if !cfg.enabled {
            return Ok(None);
        }

        let cadence = Duration::from_secs(u64::from(cfg.interval_minutes) * 60);
        if let Some(last) = *self.last_run_at.read().await {
            if now.signed_duration_since(last).to_std().unwrap_or_default() < cadence {
                return Ok(None);
            }
        }

        let summary = self.service.run_once(now).await?;
        *self.last_run_at.write().await = Some(now);
        Ok(Some(summary))
    }

    /// Spawn the polling loop. Returns a [`StreamHandle`] consistent
    /// with the other long-running streams in `IbkrState`.
    #[allow(dead_code)] // wired in lib.rs once Tauri commands land
    pub fn spawn(self: Arc<Self>) -> StreamHandle {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_task = Arc::clone(&shutdown);
        let scheduler = Arc::clone(&self);
        // Cap polls at 60s so RTH-open / first-tick boundaries land
        // promptly even when the configured cadence is much larger.
        let poll = scheduler.poll_interval.min(Duration::from_secs(60));

        let join = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(poll);
            ticker.tick().await; // drop the immediate first tick

            loop {
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                ticker.tick().await;
                if shutdown_task.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = scheduler.tick().await {
                    warn!("auto-scanner tick failed: {e}");
                }
            }
            info!("auto-scanner scheduler stopped");
        });

        StreamHandle::new("auto scanner", shutdown, join)
    }
}
