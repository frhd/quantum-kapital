// Phase 10 — public-surface helpers (Tauri commands, frontend serde,
// vintage record) that the lib doesn't yet call internally. Kept on
// the API rather than gated behind feature flags so external consumers
// land cleanly.
#![allow(dead_code, unused_imports)]

//! Phase 10 (quant-decisions) — `ParamRefitService`: monthly walk-
//! forward refit of detector parameters. Stops the implicit "trader
//! picked these once and never revisited" pattern by re-evaluating
//! each detector's free knobs against a rolling 12-month window and
//! locking the winner only when it beats the current vintage by ≥10%
//! AND meets all hard constraints.
//!
//! Three entry points:
//!
//!   - [`ParamRefitService::run_monthly`] — full sweep over every
//!     detector. Called by the monthly scheduler (and by
//!     `param_refit_run_now` for manual triggers).
//!   - [`ParamRefitService::run_for_detector`] — sweep one detector.
//!     Used by the startup backfill path when a detector has no
//!     active vintage yet (master-plan backfill trigger decision).
//!   - [`ParamRefitService::active_for`] — cheap reader for the
//!     active vintage. Threaded through `TrackerRunner` so each
//!     fired setup carries `param_vintage_id` for attribution.
//!
//! Determinism contract: [`SweepEngine`] uses a seeded RNG
//! (`rng_seed`); same seed + same backtest spec ⇒ same vintage. The
//! seed is derived from `(detector, refit_at.date())` so two refits
//! on the same calendar day for the same detector pick the same
//! candidates. The `attempted_configs_json` audit array carries
//! every config the sweep tried; reviewers can spot a winner that
//! barely beat 199 others vs one that decisively beat 5.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use thiserror::Error;
use tracing::{info, warn};

use crate::services::backtester::BacktesterError;
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::strategies::DetectorsConfig;

pub mod objective;
pub mod scheduler;
pub mod sweep;
pub mod vintage_store;

#[cfg(test)]
mod tests;

pub use objective::{ConstraintFailure, Objective, ObjectiveScore};
pub use scheduler::MonthlyRefitScheduler;
pub use sweep::{
    detector_seed, BacktesterFactory, ParamSpace, ProdBacktesterFactory, SweepCandidate,
    SweepEngine, SweepReport, BREAKOUT_DETECTOR, EPISODIC_PIVOT_DETECTOR, PARABOLIC_SHORT_DETECTOR,
};
pub use vintage_store::{LockSource, ParamVintage, VintageStore};

/// Default sweep budget — number of backtest configs evaluated per
/// detector per refit. Master-plan committed to 200 as the cap; we
/// stay below that by default to keep CI runs and dogfood refits
/// from chewing minutes of compute. The CLI / scheduler can override
/// up to the 200 ceiling.
pub const DEFAULT_SWEEP_BUDGET: u32 = 200;

/// Master-plan committed sweep window: 12 calendar months total —
/// 9 months train + 3 months OOS. Chosen so the OOS window meets
/// the "≥ 1/3 of train" overfit-mitigation guard (gotcha in phase
/// doc), while keeping the per-config backtest scope small enough
/// (3 months) for a 200-config sweep to land in a reasonable window.
pub const TRAIN_MONTHS: u32 = 9;
pub const OOS_MONTHS: u32 = 3;

/// Lock-on-improvement guard: a candidate vintage must beat the
/// active vintage's `objective_value` by this multiplicative factor
/// to unseat it. Master-plan committed: 10%. Below this threshold,
/// the active vintage stays in place to prevent churn breaking the
/// trader's mental model.
pub const LOCK_IMPROVEMENT_FACTOR: f64 = 1.10;

#[derive(Debug, Error)]
pub enum RefitError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("serde_json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("backtester: {0}")]
    Backtester(#[from] BacktesterError),
    #[error("no symbols available for sweep")]
    NoSymbols,
    #[error("unknown detector: {0}")]
    UnknownDetector(String),
}

pub type Result<T> = std::result::Result<T, RefitError>;

/// Outcome of a single detector's refit attempt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DetectorRefitOutcome {
    pub detector: String,
    /// Why the refit ended this way. `Locked` ⇒ a new vintage was
    /// written; `Held` ⇒ the candidate didn't beat the active by
    /// enough; `Skipped` ⇒ no candidate met constraints (or no bars
    /// available); `Errored` ⇒ a backtest failed mid-sweep (logged,
    /// surfaced to the eval panel).
    pub status: RefitStatus,
    /// `Some` when `status = Locked`. Carries the new active vintage
    /// for the eval panel to render without a second query.
    pub new_vintage: Option<ParamVintage>,
    /// Best objective value the sweep produced, even when not locked.
    /// `None` ⇒ no candidate met the hard constraints.
    pub best_objective: Option<f64>,
    /// Active vintage's objective at the time of the refit. `None` ⇒
    /// no prior vintage existed (first-ever refit for the detector).
    pub baseline_objective: Option<f64>,
    /// Number of configs the sweep evaluated (≤ budget).
    pub n_attempted: u32,
    /// Number of configs that met the hard constraints.
    pub n_constraints_passed: u32,
    /// Free-form note from the lock decision (e.g. "best_pf=1.42 vs
    /// baseline=1.30 → 1.09× improvement, below 1.10× threshold").
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefitStatus {
    Locked,
    Held,
    Skipped,
    Errored,
}

impl RefitStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RefitStatus::Locked => "locked",
            RefitStatus::Held => "held",
            RefitStatus::Skipped => "skipped",
            RefitStatus::Errored => "errored",
        }
    }
}

/// Aggregate report for a `run_monthly` invocation. The eval panel's
/// timeline reads this directly; the cron scheduler emits it via
/// `tracing::info!` so the operator can see the outcome of the most
/// recent refit without poking the DB.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RefitReport {
    pub refit_at: DateTime<Utc>,
    pub source: String,
    pub outcomes: Vec<DetectorRefitOutcome>,
}

/// Symbol universe + date-window inputs for a sweep. The service
/// computes these from the runtime configuration; tests inject a
/// fixed shape via [`SweepInputs::for_test`].
#[derive(Debug, Clone)]
pub struct SweepInputs {
    pub symbols: Vec<String>,
    pub train_from: NaiveDate,
    pub train_to: NaiveDate,
    pub oos_from: NaiveDate,
    pub oos_to: NaiveDate,
}

impl SweepInputs {
    /// Compute a 9-month-train + 3-month-OOS window ending on
    /// `refit_at`'s ET-local date. The OOS window is the most
    /// recent 3 calendar months; the train window is the 9 months
    /// preceding that.
    pub fn from_refit_anchor(refit_at: DateTime<Utc>, symbols: Vec<String>) -> Self {
        let anchor = crate::utils::market_calendar::et_date(refit_at);
        let oos_to = anchor;
        let oos_from = subtract_months_clamped(anchor, OOS_MONTHS);
        let train_to = oos_from
            .pred_opt()
            .expect("date arithmetic does not overflow");
        let train_from = subtract_months_clamped(train_to, TRAIN_MONTHS);
        Self {
            symbols,
            train_from,
            train_to,
            oos_from,
            oos_to,
        }
    }
}

/// Subtract `n` calendar months from `date`, clamping the day to the
/// last valid day of the resulting month. Used to roll a window
/// backward without panicking on March 31 → February 31.
fn subtract_months_clamped(date: NaiveDate, n: u32) -> NaiveDate {
    use chrono::Datelike;
    let mut year = date.year();
    let mut month = date.month() as i32 - n as i32;
    while month <= 0 {
        month += 12;
        year -= 1;
    }
    let day = date.day();
    // Try the same day; back off until it lands.
    for d in (1..=day).rev() {
        if let Some(nd) = NaiveDate::from_ymd_opt(year, month as u32, d) {
            return nd;
        }
    }
    // Should be unreachable — every month has a day 1.
    NaiveDate::from_ymd_opt(year, month as u32, 1).expect("month 1..12 always has a day 1")
}

/// Phase 10 entry point. Cheap to clone (`Arc` internals).
#[derive(Clone)]
pub struct ParamRefitService {
    db: Arc<Db>,
    factory: Arc<dyn BacktesterFactory>,
    vintage_store: VintageStore,
    /// Default symbol universe used by the sweep when the caller
    /// doesn't override. Initialized from settings; in production
    /// this is the same top-N watchlist seed list the regime service
    /// uses (P9). Defaults are loaded from
    /// `crate::services::regime::UNIVERSE`.
    default_symbols: Arc<Vec<String>>,
    /// Max configs evaluated per detector per refit. Defaults to
    /// `DEFAULT_SWEEP_BUDGET`; configurable for tests + CLI.
    sweep_budget: u32,
    /// settings.toml-derived bounds for the sweep parameter space.
    /// Used as floor/ceiling for random/grid sampling. Master-plan
    /// committed: settings.toml becomes bounds, not active params.
    bounds: Arc<DetectorsConfig>,
}

impl ParamRefitService {
    pub fn new(
        db: Arc<Db>,
        factory: Arc<dyn BacktesterFactory>,
        bounds: DetectorsConfig,
        default_symbols: Vec<String>,
    ) -> Self {
        Self {
            db: Arc::clone(&db),
            factory,
            vintage_store: VintageStore::new(db),
            default_symbols: Arc::new(default_symbols),
            sweep_budget: DEFAULT_SWEEP_BUDGET,
            bounds: Arc::new(bounds),
        }
    }

    pub fn with_sweep_budget(mut self, budget: u32) -> Self {
        self.sweep_budget = budget.clamp(1, DEFAULT_SWEEP_BUDGET);
        self
    }

    pub fn vintage_store(&self) -> VintageStore {
        self.vintage_store.clone()
    }

    /// Active vintage for `detector`, if any. Cheap (single SELECT).
    /// Returns `None` for first-run state (no vintage yet) and for
    /// unknown detectors.
    pub async fn active_for(&self, detector: &str) -> Result<Option<ParamVintage>> {
        self.vintage_store.active_for(detector).await
    }

    /// History of vintages for `detector`, newest-first. The eval
    /// panel renders this as a timeline.
    pub async fn history_for(&self, detector: &str, limit: u32) -> Result<Vec<ParamVintage>> {
        self.vintage_store.history_for(detector, limit).await
    }

    /// Snapshot of every detector's active vintage. Returns one row
    /// per detector that has ever been refitted; absent detectors
    /// fall back to `bounds` (settings.toml defaults) at runtime.
    pub async fn active_all(&self) -> Result<Vec<ParamVintage>> {
        self.vintage_store.active_all().await
    }

    /// Build a `DetectorsConfig` whose per-detector params come from
    /// the active vintages. Detectors without a vintage fall back
    /// to the bounds config. Called at app boot to seed
    /// `registry_from_config(...)` and after every successful refit
    /// to give the next runner-tick the new active params.
    pub async fn effective_detectors_config(&self) -> Result<DetectorsConfig> {
        let mut cfg = (*self.bounds).clone();
        let actives = self.active_all().await?;
        for vintage in actives {
            if let Err(e) = vintage_store::apply_vintage_to_config(&mut cfg, &vintage) {
                warn!(
                    "param_refit: vintage {} for {} failed to apply ({e}); falling back to bounds",
                    vintage.vintage_id, vintage.detector
                );
            }
        }
        Ok(cfg)
    }

    /// Run a refit for every detector. Used by the monthly cron
    /// (`source = "cron"`) and by `param_refit_run_now`
    /// (`source = "manual"`). Each detector is independent — a
    /// failure on one doesn't block the others.
    pub async fn run_monthly(&self, source: LockSource) -> Result<RefitReport> {
        self.run_subset(self.all_detectors(), source).await
    }

    /// One-shot refit for a single detector. Used by the startup
    /// backfill path (`source = "backfill"`) when a detector ships
    /// without an active vintage so the runner doesn't fire setups
    /// against bounds-only params.
    pub async fn run_for_detector(
        &self,
        detector: &str,
        source: LockSource,
    ) -> Result<DetectorRefitOutcome> {
        let outcomes = self.run_subset(vec![detector.to_string()], source).await?;
        outcomes
            .outcomes
            .into_iter()
            .next()
            .ok_or_else(|| RefitError::UnknownDetector(detector.to_string()))
    }

    /// Run the backfill-on-startup path for any detector that has
    /// no active vintage. Returns `None` if every detector already
    /// has an active vintage (the steady state).
    pub async fn backfill_missing(&self) -> Result<Option<RefitReport>> {
        let actives = self.active_all().await?;
        let active_set: std::collections::HashSet<String> =
            actives.into_iter().map(|v| v.detector).collect();
        let missing: Vec<String> = self
            .all_detectors()
            .into_iter()
            .filter(|d| !active_set.contains(d))
            .collect();
        if missing.is_empty() {
            return Ok(None);
        }
        info!(
            "param_refit: backfilling {} detector(s) without active vintage: {missing:?}",
            missing.len()
        );
        Ok(Some(self.run_subset(missing, LockSource::Backfill).await?))
    }

    fn all_detectors(&self) -> Vec<String> {
        vec![
            BREAKOUT_DETECTOR.to_string(),
            EPISODIC_PIVOT_DETECTOR.to_string(),
            PARABOLIC_SHORT_DETECTOR.to_string(),
        ]
    }

    async fn run_subset(&self, detectors: Vec<String>, source: LockSource) -> Result<RefitReport> {
        let refit_at = Utc::now();
        let inputs = SweepInputs::from_refit_anchor(refit_at, (*self.default_symbols).clone());
        if inputs.symbols.is_empty() {
            return Err(RefitError::NoSymbols);
        }
        let mut outcomes = Vec::with_capacity(detectors.len());
        for detector in detectors {
            let outcome = self
                .run_one(&detector, &inputs, refit_at, source)
                .await
                .unwrap_or_else(|e| {
                    warn!("param_refit: detector {detector} errored: {e}");
                    DetectorRefitOutcome {
                        detector: detector.clone(),
                        status: RefitStatus::Errored,
                        new_vintage: None,
                        best_objective: None,
                        baseline_objective: None,
                        n_attempted: 0,
                        n_constraints_passed: 0,
                        note: format!("error: {e}"),
                    }
                });
            outcomes.push(outcome);
        }
        Ok(RefitReport {
            refit_at,
            source: source.as_str().to_string(),
            outcomes,
        })
    }

    async fn run_one(
        &self,
        detector: &str,
        inputs: &SweepInputs,
        refit_at: DateTime<Utc>,
        source: LockSource,
    ) -> Result<DetectorRefitOutcome> {
        let space = sweep::space_for(detector, &self.bounds)
            .ok_or_else(|| RefitError::UnknownDetector(detector.to_string()))?;
        let active = self.active_for(detector).await?;
        let baseline_objective = active.as_ref().map(|v| v.objective_value);
        let engine = SweepEngine::new(
            detector.to_string(),
            space,
            self.sweep_budget,
            detector_seed(detector, refit_at),
        );
        let report = engine
            .run(self.factory.as_ref(), inputs, &self.bounds)
            .await?;

        let lock_threshold = baseline_objective.map(|b| b * LOCK_IMPROVEMENT_FACTOR);
        let n_attempted = report.candidates.len() as u32;
        let n_constraints_passed = report
            .candidates
            .iter()
            .filter(|c| c.score.is_some())
            .count() as u32;
        let best = report
            .candidates
            .iter()
            .filter_map(|c| c.score.as_ref().map(|s| (c, s)))
            .max_by(|a, b| {
                a.1.value
                    .partial_cmp(&b.1.value)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let best_objective = best.as_ref().map(|(_c, s)| s.value);

        match best {
            None => Ok(DetectorRefitOutcome {
                detector: detector.to_string(),
                status: RefitStatus::Skipped,
                new_vintage: None,
                best_objective,
                baseline_objective,
                n_attempted,
                n_constraints_passed,
                note: format!(
                    "no candidate met hard constraints (n_attempted={n_attempted}, n_constraints_passed={n_constraints_passed})"
                ),
            }),
            Some((winner, score)) => {
                let beats_threshold = match lock_threshold {
                    None => true, // first-ever refit: always lock
                    Some(t) => score.value >= t,
                };
                if !beats_threshold {
                    let baseline = baseline_objective.unwrap_or(0.0);
                    let ratio = if baseline > 0.0 { score.value / baseline } else { 0.0 };
                    return Ok(DetectorRefitOutcome {
                        detector: detector.to_string(),
                        status: RefitStatus::Held,
                        new_vintage: None,
                        best_objective,
                        baseline_objective,
                        n_attempted,
                        n_constraints_passed,
                        note: format!(
                            "best={:.3} vs baseline={:.3} ({:.2}× improvement, below {:.2}× lock threshold)",
                            score.value, baseline, ratio, LOCK_IMPROVEMENT_FACTOR
                        ),
                    });
                }
                let vintage = self
                    .vintage_store
                    .lock_new(
                        detector,
                        &winner.params_json,
                        score.value,
                        score.n_trades as i64,
                        inputs,
                        refit_at,
                        source,
                        &report.candidates,
                        None,
                    )
                    .await?;
                let baseline_text = baseline_objective
                    .map(|b| format!("baseline={b:.3}, "))
                    .unwrap_or_default();
                Ok(DetectorRefitOutcome {
                    detector: detector.to_string(),
                    status: RefitStatus::Locked,
                    new_vintage: Some(vintage),
                    best_objective,
                    baseline_objective,
                    n_attempted,
                    n_constraints_passed,
                    note: format!(
                        "{baseline_text}best={:.3} (n_oos_trades={}, sharpe={:.2}, expectancy={:.2}R)",
                        score.value, score.n_trades, score.sharpe, score.expectancy_r
                    ),
                })
            }
        }
    }

    /// Admin path: lock a manual params override without running a
    /// sweep. The vintage is recorded with `source = manual`,
    /// `attempted_configs_json = []`, and the supplied `notes`. The
    /// next refit's lock-on-improvement check uses
    /// `manual_objective` as the baseline — supply the operator's
    /// estimate (or 0.0 to make any successful refit unseat it).
    pub async fn lock_manual(
        &self,
        detector: &str,
        params_json: serde_json::Value,
        objective_value: f64,
        oos_n_trades: i64,
        notes: Option<String>,
    ) -> Result<ParamVintage> {
        let now = Utc::now();
        // Manual locks don't have a sweep window — stamp a 1-day
        // train/OOS pair to keep the schema NOT NULL constraints
        // happy. The eval panel renders manual rows distinctly.
        let inputs = SweepInputs {
            symbols: Vec::new(),
            train_from: crate::utils::market_calendar::et_date(now),
            train_to: crate::utils::market_calendar::et_date(now),
            oos_from: crate::utils::market_calendar::et_date(now),
            oos_to: crate::utils::market_calendar::et_date(now),
        };
        self.vintage_store
            .lock_new(
                detector,
                &params_json,
                objective_value,
                oos_n_trades,
                &inputs,
                now,
                LockSource::Manual,
                &[],
                notes,
            )
            .await
    }
}

impl std::fmt::Debug for ParamRefitService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParamRefitService")
            .field("sweep_budget", &self.sweep_budget)
            .field("default_symbols_n", &self.default_symbols.len())
            .finish_non_exhaustive()
    }
}
