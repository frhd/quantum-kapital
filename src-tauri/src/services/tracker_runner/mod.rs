//! Phase 10 — `TrackerRunner` glues bars + news + the detector registry
//! together so a single Tauri command can evaluate one symbol (or the
//! whole watchlist) end-to-end.
//!
//! The runner is intentionally small: it owns no state of its own
//! beyond `Arc` handles to its dependencies. Ownership of bars / news
//! data lives in `OwnedMarketContext`, which is converted to a
//! borrowed `MarketContext<'_>` at the detector call site so the
//! existing detector trait can keep its no-allocation contract.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::warn;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::error::Result as IbkrResult;
use crate::ibkr::types::data_tier::DataTier;
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::news::{NewsItem, NewsVerdict};
use crate::ibkr::types::tracker::{AlertKind, Setup, TrackerStatus};
use crate::services::alerts::record_alert;
use crate::services::event_calendar::{Blackout, EventCalendarService};
use crate::services::historical_data_service::{HistoricalDataService, Lookback};
use crate::services::news_provider::NewsProvider;
use crate::services::risk_engine::RiskEngine;
use crate::services::thesis_generator::{ThesisContext, ThesisGenerator};
use crate::services::tracker_service::{Result as TrackerResult, TrackerService};
use crate::services::tracker_state_machine::TrackerStateMachine;
use crate::storage::Db;
use crate::strategies::{DetectorRegistry, DetectorsConfig, MarketContext, SkipReason};

#[cfg(test)]
mod tests;

/// Default duplicate-suppression window. Re-running detectors against
/// the same `(symbol, strategy, direction)` within this window does
/// not re-insert a row. Phase 10 picks 24h as a conservative single-
/// day guard; Phase 12 will revisit when the status state machine
/// owns "fresh vs stale" semantics.
pub const DUPLICATE_WINDOW: ChronoDuration = ChronoDuration::hours(24);

const DAILY_LOOKBACK_DAYS: u32 = 200;
const INTRADAY_BAR_SIZE: BarSize = BarSize::Min15;
const NEWS_LOOKBACK_HOURS: u32 = 24;

// ---------------- traits (test seams) ----------------

/// Narrow seam for fetching historical bars. Production wiring uses
/// the real `HistoricalDataService`; tests use a hand-rolled mock so
/// they don't have to stand up the full SQLite cache + IBKR client
/// stack just to hand bars to the runner.
#[async_trait]
pub trait BarsFetcher: Send + Sync {
    async fn fetch(
        &self,
        symbol: &str,
        bar_size: BarSize,
        lookback: Lookback,
    ) -> IbkrResult<Vec<HistoricalBar>>;
}

#[async_trait]
impl BarsFetcher for HistoricalDataService {
    async fn fetch(
        &self,
        symbol: &str,
        bar_size: BarSize,
        lookback: Lookback,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        self.fetch_bars(symbol, bar_size, lookback).await
    }
}

// News fetching goes through `Arc<dyn NewsProvider>` directly. Phase 7
// (AV strip-out) removed the dedicated `NewsFetcher` shim — the runner
// applies the best-effort policy at the call site
// ([`TrackerRunner::context_for`]), logging + collapsing every
// [`crate::services::news_provider::NewsError`] variant to
// `Vec::new()` so a transport failure never short-circuits a runner
// pass.

// ---------------- types ----------------

/// Owned counterpart to [`MarketContext`]. The runner returns these
/// because async fetches happen on a different stack frame than the
/// detector calls; the borrowed `MarketContext` is constructed at the
/// dispatch site via [`OwnedMarketContext::as_borrowed`].
#[derive(Debug, Clone)]
pub struct OwnedMarketContext {
    pub symbol: String,
    pub daily_bars: Vec<HistoricalBar>,
    pub intraday_bars: Option<Vec<HistoricalBar>>,
    pub recent_news: Vec<NewsItem>,
    pub news_verdict: Option<NewsVerdict>,
    /// Tier the runner saw for the active IBKR connection at the
    /// moment this context was assembled. Defaults to `Unknown` when
    /// no tier source is wired.
    pub data_tier: DataTier,
    pub now: DateTime<Utc>,
}

impl OwnedMarketContext {
    pub fn as_borrowed(&self) -> MarketContext<'_> {
        MarketContext {
            symbol: &self.symbol,
            daily_bars: &self.daily_bars,
            intraday_bars: self.intraday_bars.as_deref(),
            fundamentals: None,
            recent_news: &self.recent_news,
            news_verdict: self.news_verdict.as_ref(),
            current_quote: None,
            data_tier: self.data_tier,
            now: self.now,
        }
    }
}

/// Result of a single-symbol pass through the registry. Holds both the
/// persisted setups and any error that arose during context-gathering
/// so the batch-runner can surface per-symbol failures without
/// short-circuiting the whole sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub symbol: String,
    pub setups: Vec<Setup>,
    pub error: Option<String>,
}

// ---------------- runner ----------------

#[derive(Clone)]
pub struct TrackerRunner {
    db: Arc<Db>,
    tracker: Arc<TrackerService>,
    state_machine: Arc<TrackerStateMachine>,
    emitter: Arc<EventEmitter>,
    bars: Arc<dyn BarsFetcher>,
    news: Arc<dyn NewsProvider>,
    registry: Arc<DetectorRegistry>,
    /// Phase 17 — optional. When wired, runs after each persisted setup
    /// to attach an LLM-generated thesis. When `None`, the runner emits
    /// `SetupDetected { thesis: None }` exactly as Phase 15 did.
    thesis_generator: Option<Arc<ThesisGenerator>>,
    /// Source of the live `DataTier` for context construction. When
    /// `None`, contexts default to `DataTier::Unknown` — preserving
    /// pre-data-tier behavior for tests that don't care.
    data_tier: Option<Arc<RwLock<DataTier>>>,
    /// Quant-decisions Phase 1 — optional risk engine. When wired,
    /// every newly-persisted setup is sized immediately, the sizing
    /// is written back to the row, and `SetupSized` fires before
    /// `SetupDetected`. When `None`, the runner is sizing-blind
    /// (back-compat for tests).
    risk_engine: Option<Arc<RiskEngine>>,
    /// Quant-decisions Phase 5 — optional event-blackout gate. When
    /// wired, every detector hit is checked against the per-detector
    /// blackout policy before persistence. Hits inside an event window
    /// land as `setups` rows with `skipped_reason` set, fire
    /// `SetupSkipped`, and bypass the sizing / state-machine / thesis
    /// paths. When `None`, the runner is gate-blind (back-compat).
    event_calendar: Option<Arc<EventCalendarService>>,
    /// Phase 5 — detector-config snapshot used to look up the blackout
    /// policy for the candidate's `strategy`. Held alongside the gate
    /// so a single `with_event_calendar` call wires both pieces.
    detectors_config: Option<DetectorsConfig>,
}

impl TrackerRunner {
    pub fn new(
        db: Arc<Db>,
        tracker: Arc<TrackerService>,
        state_machine: Arc<TrackerStateMachine>,
        emitter: Arc<EventEmitter>,
        bars: Arc<dyn BarsFetcher>,
        news: Arc<dyn NewsProvider>,
        registry: Arc<DetectorRegistry>,
    ) -> Self {
        Self {
            db,
            tracker,
            state_machine,
            emitter,
            bars,
            news,
            registry,
            thesis_generator: None,
            data_tier: None,
            risk_engine: None,
            event_calendar: None,
            detectors_config: None,
        }
    }

    /// Attach a [`RiskEngine`]. Once wired, every persisted setup is
    /// sized + persisted with sizing fields populated, and
    /// `SetupSized` fires before `SetupDetected`.
    pub fn with_risk_engine(mut self, engine: Arc<RiskEngine>) -> Self {
        self.risk_engine = Some(engine);
        self
    }

    /// Phase 5 — attach the event-blackout gate. Detector hits inside
    /// an earnings or FOMC window get persisted as skipped setups
    /// (`skipped_reason` set, `SetupSkipped` fired) and bypass sizing /
    /// state-machine / thesis. `cfg` is the detector-config snapshot
    /// used to look up the per-strategy blackout policy.
    pub fn with_event_calendar(
        mut self,
        gate: Arc<EventCalendarService>,
        cfg: DetectorsConfig,
    ) -> Self {
        self.event_calendar = Some(gate);
        self.detectors_config = Some(cfg);
        self
    }

    /// Attach a [`ThesisGenerator`]. With one wired, `run_for` skips the
    /// `thesis: None` event when generation succeeds — the generator
    /// emits `SetupDetected` itself with the populated thesis.
    pub fn with_thesis_generator(mut self, generator: Arc<ThesisGenerator>) -> Self {
        self.thesis_generator = Some(generator);
        self
    }

    /// Attach a `DataTier` source. `IbkrState.data_tier` is wired in by
    /// `lib.rs` so `MarketContext.data_tier` reflects what the active
    /// IBKR connection is actually delivering. Without one, contexts
    /// stay `DataTier::Unknown` — tier-gated detectors should treat
    /// `Unknown` as "don't run".
    pub fn with_data_tier(mut self, data_tier: Arc<RwLock<DataTier>>) -> Self {
        self.data_tier = Some(data_tier);
        self
    }

    async fn read_data_tier(&self) -> DataTier {
        match &self.data_tier {
            Some(source) => *source.read().await,
            None => DataTier::Unknown,
        }
    }

    /// Gather a [`MarketContext`] for `symbol`. Daily bars are mandatory
    /// (a fetch failure here propagates as an error); intraday bars
    /// and news are best-effort and degrade to `None` / empty on
    /// failure. Fundamentals and live quotes are intentionally `None`
    /// for Phase 10 — no current detector reads them.
    pub async fn context_for(&self, symbol: &str) -> IbkrResult<OwnedMarketContext> {
        let symbol_norm = symbol.to_uppercase();

        let daily_bars = self
            .bars
            .fetch(
                &symbol_norm,
                BarSize::Day1,
                Lookback::Days(DAILY_LOOKBACK_DAYS),
            )
            .await?;

        let intraday_bars = match self
            .bars
            .fetch(&symbol_norm, INTRADAY_BAR_SIZE, Lookback::Days(1))
            .await
        {
            Ok(bars) if bars.is_empty() => None,
            Ok(bars) => Some(bars),
            Err(e) => {
                warn!("intraday bars fetch failed for {symbol_norm} (best-effort): {e}");
                None
            }
        };

        let recent_news = match self.news.fetch(&symbol_norm, NEWS_LOOKBACK_HOURS).await {
            Ok(items) => items,
            Err(e) => {
                warn!("news fetch failed for {symbol_norm} (best-effort): {e}");
                Vec::new()
            }
        };

        Ok(OwnedMarketContext {
            symbol: symbol_norm,
            daily_bars,
            intraday_bars,
            recent_news,
            news_verdict: None,
            data_tier: self.read_data_tier().await,
            now: Utc::now(),
        })
    }

    /// Run all registered detectors against `symbol` and persist the
    /// hits. Returns the persisted rows; misses (None outcomes) and
    /// detector errors are logged but not returned. Touches the
    /// ticker's `last_checked_at` after a successful pass.
    pub async fn run_for(&self, symbol: &str) -> IbkrResult<Vec<Setup>> {
        let ctx_owned = self.context_for(symbol).await?;
        let ctx = ctx_owned.as_borrowed();
        let outcomes = self.registry.evaluate_all(&ctx).await;

        let mut persisted = Vec::new();
        for outcome in outcomes {
            match outcome.result {
                Ok(Some(candidate)) => {
                    // Phase 5 — event-blackout gate. Run before
                    // persistence so the gate can divert the hit into
                    // a `skipped` row instead of a sized one. The
                    // dedup window still applies to skipped rows so
                    // we don't spam the SkippedSetupsPanel with the
                    // same blackout for one symbol every tick.
                    let gate_outcome = self
                        .check_blackout(&ctx_owned.symbol, &candidate, ctx_owned.now)
                        .await;
                    match gate_outcome {
                        Ok(Some(blackout)) => {
                            match self
                                .persist_skipped_with_dedup(
                                    &ctx_owned.symbol,
                                    &candidate,
                                    &blackout,
                                )
                                .await
                            {
                                Ok(Some(skipped)) => {
                                    let kind = blackout.kind.as_str().to_string();
                                    let _ = self
                                        .emitter
                                        .emit(AppEvent::SetupSkipped {
                                            setup_id: skipped.id,
                                            symbol: skipped.symbol.clone(),
                                            strategy: skipped.strategy.clone(),
                                            kind,
                                            reason: blackout.reason.clone(),
                                        })
                                        .await;
                                }
                                Ok(None) => {
                                    // Recent duplicate skipped row — silent.
                                }
                                Err(e) => warn!(
                                    "failed to persist skipped setup for {} ({}): {e}",
                                    ctx_owned.symbol, outcome.detector
                                ),
                            }
                            continue;
                        }
                        Ok(None) => {
                            // No blackout — fall through to the normal
                            // sized-and-persisted path.
                        }
                        Err(e) => {
                            warn!(
                                "event_calendar lookup failed for {} ({}): {e} — \
                                 proceeding without gate (fail-open)",
                                ctx_owned.symbol, outcome.detector
                            );
                        }
                    }
                    match self.persist_with_dedup(&ctx_owned.symbol, &candidate).await {
                        Ok(Some(mut setup)) => {
                            // Quant-decisions Phase 1 — size the setup
                            // immediately. Failures don't kill the row;
                            // the UI surfaces an "ungated" warning when
                            // sizing is None.
                            if let Some(engine) = &self.risk_engine {
                                match engine.size_for_candidate(&candidate).await {
                                    Ok((sizing, _snapshot)) => {
                                        match self
                                            .tracker
                                            .update_setup_sizing(setup.id, &sizing)
                                            .await
                                        {
                                            Ok(refreshed) => {
                                                setup = refreshed;
                                                let _ = self
                                                    .emitter
                                                    .emit(AppEvent::SetupSized {
                                                        setup_id: setup.id,
                                                        symbol: setup.symbol.clone(),
                                                        sizing: sizing.clone(),
                                                    })
                                                    .await;
                                            }
                                            Err(e) => warn!(
                                                "update_setup_sizing failed for {} setup#{}: {e}",
                                                ctx_owned.symbol, setup.id
                                            ),
                                        }
                                    }
                                    Err(e) => warn!(
                                        "risk_engine sizing failed for {} setup#{}: {e}",
                                        ctx_owned.symbol, setup.id
                                    ),
                                }
                            }
                            // Phase 12: hand the persisted hit to the state
                            // machine so the ticker flips into SetupActive
                            // (and gets its in_play_until extended). Failures
                            // here are surfaced as warnings — the setup row
                            // is already persisted, so the caller still gets
                            // the data back.
                            if let Err(e) = self
                                .state_machine
                                .on_setup_detected(&ctx_owned.symbol, setup.id)
                                .await
                            {
                                warn!(
                                    "state-machine on_setup_detected failed for {}: {e}",
                                    ctx_owned.symbol
                                );
                            }
                            // Phase 21: record a `detected` alert so the
                            // AlertFeed can surface this hit even if the
                            // user missed the toast. The dedup window in
                            // `record_alert` collapses the runner's
                            // first-emit + thesis-generated re-emit into
                            // a single row.
                            if let Err(e) = record_alert(
                                &self.db,
                                setup.id,
                                AlertKind::Detected,
                                serde_json::json!({
                                    "symbol": setup.symbol,
                                    "strategy": setup.strategy,
                                    "direction": setup.direction,
                                    "trigger_price": setup.trigger_price,
                                    "stop_price": setup.stop_price,
                                    "detected_at": setup.detected_at,
                                }),
                            )
                            .await
                            {
                                warn!("record_alert(detected) failed for setup#{}: {e}", setup.id);
                            }
                            // Phase 17: if a thesis generator is wired, let
                            // it own the `SetupDetected` emission (with the
                            // populated thesis). On success it persists the
                            // thesis to the row + emits SetupDetected with
                            // Some(thesis). On any other outcome (graceful
                            // fallback, idempotent skip, error) we fall back
                            // to the Phase 15 emit so the frontend still
                            // updates without a manual refresh.
                            let mut thesis_emitted = false;
                            if let Some(gen) = &self.thesis_generator {
                                let thesis_ctx = ThesisContext {
                                    daily_bars: &ctx_owned.daily_bars,
                                    recent_news: &ctx_owned.recent_news,
                                };
                                match gen.generate(&setup, &thesis_ctx).await {
                                    Ok(Some(_)) => {
                                        thesis_emitted = true;
                                    }
                                    Ok(None) => {
                                        // Idempotent skip or graceful LLM fallback.
                                    }
                                    Err(e) => {
                                        warn!(
                                            "thesis generator failed for {} setup#{}: {e}",
                                            ctx_owned.symbol, setup.id
                                        );
                                    }
                                }
                            }
                            if !thesis_emitted {
                                let _ = self
                                    .emitter
                                    .emit(AppEvent::SetupDetected {
                                        setup: Box::new(setup.clone()),
                                        thesis: None,
                                    })
                                    .await;
                            }
                            persisted.push(setup);
                        }
                        Ok(None) => {
                            // Recent duplicate — silently skip.
                        }
                        Err(e) => {
                            warn!(
                                "failed to persist setup for {} ({}): {e}",
                                ctx_owned.symbol, outcome.detector
                            );
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        "detector {} failed for {}: {e}",
                        outcome.detector, ctx_owned.symbol
                    );
                }
            }
        }

        if let Err(e) = self.tracker.touch_last_checked(&ctx_owned.symbol).await {
            warn!("touch_last_checked failed for {}: {e}", ctx_owned.symbol);
        }

        Ok(persisted)
    }

    /// Iterate the watchlist (excluding `CoolDown` rows) and run each
    /// in turn. Per-symbol failures are captured into the matching
    /// `RunResult` and never short-circuit the batch.
    pub async fn run_all(&self) -> TrackerResult<Vec<RunResult>> {
        let watchlist = self.tracker.list(None).await?;
        let mut results = Vec::with_capacity(watchlist.len());
        for ticker in watchlist {
            if matches!(ticker.status, TrackerStatus::CoolDown) {
                continue;
            }
            let symbol = ticker.symbol.clone();
            let result = match self.run_for(&symbol).await {
                Ok(setups) => RunResult {
                    symbol,
                    setups,
                    error: None,
                },
                Err(e) => RunResult {
                    symbol,
                    setups: Vec::new(),
                    error: Some(e.to_string()),
                },
            };
            results.push(result);
        }
        Ok(results)
    }

    async fn persist_with_dedup(
        &self,
        symbol: &str,
        candidate: &crate::strategies::SetupCandidate,
    ) -> TrackerResult<Option<Setup>> {
        let existing = self
            .tracker
            .recent_duplicate(
                symbol,
                candidate.strategy,
                candidate.direction,
                DUPLICATE_WINDOW,
            )
            .await?;
        if existing.is_some() {
            return Ok(None);
        }
        let row = self.tracker.insert_setup(symbol, candidate).await?;
        Ok(Some(row))
    }

    /// Phase 5 — `Some(Blackout)` if the configured gate refuses this
    /// candidate; `None` if the gate is unwired, the strategy has no
    /// blackout policy, or the policy lets the candidate through.
    async fn check_blackout(
        &self,
        symbol: &str,
        candidate: &crate::strategies::SetupCandidate,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<Blackout>, crate::services::event_calendar::EventCalendarError> {
        let gate = match &self.event_calendar {
            Some(g) => g,
            None => return Ok(None),
        };
        let cfg = match &self.detectors_config {
            Some(c) => c,
            None => return Ok(None),
        };
        let policy = cfg.blackout_policy_for(candidate.strategy);
        gate.is_blackout(symbol, now, &policy).await
    }

    /// Phase 5 — write a skipped setup row. Same dedup window as
    /// fired setups so the panel doesn't accumulate identical skips.
    async fn persist_skipped_with_dedup(
        &self,
        symbol: &str,
        candidate: &crate::strategies::SetupCandidate,
        blackout: &Blackout,
    ) -> TrackerResult<Option<Setup>> {
        let existing = self
            .tracker
            .recent_duplicate(
                symbol,
                candidate.strategy,
                candidate.direction,
                DUPLICATE_WINDOW,
            )
            .await?;
        if existing.is_some() {
            return Ok(None);
        }
        let reason = match blackout.kind {
            crate::services::event_calendar::BlackoutKind::Earnings => SkipReason::EarningsBlackout,
            crate::services::event_calendar::BlackoutKind::Fomc => SkipReason::FomcBlackout,
        };
        let window_json = serde_json::to_value(blackout)
            .unwrap_or_else(|_| serde_json::json!({ "kind": blackout.kind.as_str() }));
        let row = self
            .tracker
            .insert_skipped_setup(symbol, candidate, reason, window_json)
            .await?;
        Ok(Some(row))
    }
}
