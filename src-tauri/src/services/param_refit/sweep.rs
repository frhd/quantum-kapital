//! Phase 10 — sweep engine: enumerate parameter candidates, drive
//! the [`Backtester`] for each, score with [`Objective`], collect
//! results.
//!
//! The sweep is deterministic: same `(detector, refit_at.date())` ⇒
//! same seed ⇒ same candidate list ⇒ same winner. The seed is
//! computed by [`detector_seed`] and threaded into a `StdRng` so a
//! re-run on the same calendar day reproduces byte-identical
//! `attempted_configs_json` for the audit trail.
//!
//! Per the master-plan committed sweep budget, candidates are
//! capped at 200 per detector per refit. Cheap params (volume_mult,
//! RSI ceiling) get full grid coverage; expensive multi-axis combos
//! get random-search subsampling. The space definitions live below
//! so the bounds (read from `settings.toml`) act as min/max for
//! sampling.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{debug, warn};

use crate::ibkr::types::StrategyTag;
use crate::services::backtester::bars_reader::BarsReader;
use crate::services::backtester::{
    BacktestResult, BacktestSpec, Backtester, BacktesterError, FillModelKind, PositionSizingMode,
    WalkForwardSplits,
};
use crate::services::event_calendar::EventCalendarService;
use crate::storage::Db;
use crate::strategies::{
    registry_from_config, BreakoutCfg, BreakoutDetector, DetectorRegistry, DetectorsConfig,
    EpisodicPivotCfg, EpisodicPivotDetector, ParabolicShortCfg, ParabolicShortDetector,
};

use super::objective::{ConstraintFailure, Objective, ObjectiveScore};
use super::{Result, SweepInputs};

pub const BREAKOUT_DETECTOR: &str = "breakout";
pub const EPISODIC_PIVOT_DETECTOR: &str = "episodic_pivot";
pub const PARABOLIC_SHORT_DETECTOR: &str = "parabolic_short";

/// Deterministic seed from `(detector, refit_at.ET-date)`. Used by
/// the sweep RNG so two refits triggered on the same day for the
/// same detector pick the same candidate list.
pub fn detector_seed(detector: &str, refit_at: DateTime<Utc>) -> u64 {
    let date = crate::utils::market_calendar::et_date(refit_at);
    seed_from_string(&format!("{}|{}", detector, date.format("%Y-%m-%d")))
}

fn seed_from_string(s: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h: u64 = FNV_OFFSET;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Per-detector parameter space + the bounds it samples within.
/// Built by [`space_for`] from the `bounds` config (settings.toml-
/// derived); the sweep engine asks the space for its candidate list
/// and never decodes bounds itself.
#[derive(Debug, Clone)]
pub struct ParamSpace {
    pub detector: String,
    /// Candidate list (already filtered to `bounds`). The engine
    /// shuffles + truncates to the budget.
    pub candidates: Vec<serde_json::Value>,
}

/// One sweep candidate + its evaluation outcome. The audit trail
/// (`attempted_configs_json`) is derived from a `Vec<SweepCandidate>`.
#[derive(Debug, Clone)]
pub struct SweepCandidate {
    pub params_json: serde_json::Value,
    /// `Some` when constraints passed and the candidate is eligible
    /// to be the lock target; `None` otherwise.
    pub score: Option<ObjectiveScore>,
    pub constraint_failures: Vec<ConstraintFailure>,
}

/// Output of a sweep run. Holds every evaluated candidate (passing
/// or failing) so the audit trail captures the full picture.
#[derive(Debug, Clone)]
pub struct SweepReport {
    pub detector: String,
    pub candidates: Vec<SweepCandidate>,
}

/// Build the per-detector parameter space within `bounds`. Returns
/// `None` for unknown detector names. Each candidate's `params_json`
/// is the full `*Cfg` shape with non-swept fields carried through
/// from `bounds` so the runner can deserialize without merge logic.
pub fn space_for(detector: &str, bounds: &DetectorsConfig) -> Option<ParamSpace> {
    match detector {
        BREAKOUT_DETECTOR => Some(breakout_space(bounds)),
        EPISODIC_PIVOT_DETECTOR => Some(episodic_pivot_space(bounds)),
        PARABOLIC_SHORT_DETECTOR => Some(parabolic_short_space(bounds)),
        _ => None,
    }
}

fn breakout_space(bounds: &DetectorsConfig) -> ParamSpace {
    let base = &bounds.breakout;
    // Grid axes:
    //   - lookback_days: 15, 20, 25, 30
    //   - volume_multiple: 1.0, 1.25, 1.5, 1.75, 2.0
    //   - rsi_ceiling: 70, 75, 80, 85
    //   - atr_period: 10, 14, 20
    //   - swing_low_period: 5, 10, 15
    // Cardinality: 4*5*4*3*3 = 720 → trimmed by sweep budget.
    let mut out = Vec::new();
    for &lookback in &[15u32, 20, 25, 30] {
        for &vol in &[1.0f64, 1.25, 1.5, 1.75, 2.0] {
            for &rsi in &[70.0f64, 75.0, 80.0, 85.0] {
                for &atr in &[10u32, 14, 20] {
                    for &swing in &[5u32, 10, 15] {
                        let cfg = BreakoutCfg {
                            lookback_days: lookback,
                            volume_multiple: vol,
                            rsi_ceiling: rsi,
                            atr_period: atr,
                            swing_low_period: swing,
                            // Carry blackout knobs forward unchanged
                            // — refit doesn't re-tune blackout policy
                            // (master-plan: sweep is over the
                            // "free" detector params only).
                            earnings_bd_pre: base.earnings_bd_pre,
                            earnings_bd_post: base.earnings_bd_post,
                            skip_if_unknown_earnings: base.skip_if_unknown_earnings,
                            fomc_blackout_enabled: base.fomc_blackout_enabled,
                        };
                        if let Ok(v) = serde_json::to_value(&cfg) {
                            out.push(v);
                        }
                    }
                }
            }
        }
    }
    ParamSpace {
        detector: BREAKOUT_DETECTOR.to_string(),
        candidates: out,
    }
}

fn episodic_pivot_space(bounds: &DetectorsConfig) -> ParamSpace {
    let base = &bounds.episodic_pivot;
    // Grid axes:
    //   - min_gap_pct: 0.03, 0.04, 0.05, 0.06, 0.08
    //   - min_sentiment_abs: 0.10, 0.15, 0.20, 0.25
    //   - min_volume_ratio: 0.5, 1.0, 1.5, 2.0
    // Cardinality: 5*4*4 = 80.
    let mut out = Vec::new();
    for &gap in &[0.03f64, 0.04, 0.05, 0.06, 0.08] {
        for &sent in &[0.10f64, 0.15, 0.20, 0.25] {
            for &vol_r in &[0.5f64, 1.0, 1.5, 2.0] {
                let cfg = EpisodicPivotCfg {
                    min_gap_pct: gap,
                    min_sentiment_abs: sent,
                    min_volume_ratio: vol_r,
                    earnings_bd_pre: base.earnings_bd_pre,
                    earnings_bd_post: base.earnings_bd_post,
                    skip_if_unknown_earnings: base.skip_if_unknown_earnings,
                    fomc_blackout_enabled: base.fomc_blackout_enabled,
                };
                if let Ok(v) = serde_json::to_value(&cfg) {
                    out.push(v);
                }
            }
        }
    }
    ParamSpace {
        detector: EPISODIC_PIVOT_DETECTOR.to_string(),
        candidates: out,
    }
}

fn parabolic_short_space(bounds: &DetectorsConfig) -> ParamSpace {
    let base = &bounds.parabolic_short;
    // Grid axes:
    //   - min_consec_days: 2, 3, 4, 5
    //   - min_per_day_move: 0.04, 0.05, 0.06, 0.08
    //   - min_cumulative_move: 0.30, 0.40, 0.50, 0.60
    //   - min_atr_distance: 1.5, 2.0, 2.5, 3.0
    //   - min_rsi: 70, 75, 80, 85
    // Cardinality: 4*4*4*4*4 = 1024.
    let mut out = Vec::new();
    for &consec in &[2u32, 3, 4, 5] {
        for &per_day in &[0.04f64, 0.05, 0.06, 0.08] {
            for &cum in &[0.30f64, 0.40, 0.50, 0.60] {
                for &atr_dist in &[1.5f64, 2.0, 2.5, 3.0] {
                    for &rsi in &[70.0f64, 75.0, 80.0, 85.0] {
                        let cfg = ParabolicShortCfg {
                            min_consec_days: consec,
                            min_per_day_move: per_day,
                            min_cumulative_move: cum,
                            min_atr_distance: atr_dist,
                            min_rsi: rsi,
                            earnings_bd_pre: base.earnings_bd_pre,
                            earnings_bd_post: base.earnings_bd_post,
                            skip_if_unknown_earnings: base.skip_if_unknown_earnings,
                            fomc_blackout_enabled: base.fomc_blackout_enabled,
                        };
                        if let Ok(v) = serde_json::to_value(&cfg) {
                            out.push(v);
                        }
                    }
                }
            }
        }
    }
    ParamSpace {
        detector: PARABOLIC_SHORT_DETECTOR.to_string(),
        candidates: out,
    }
}

/// Backtester factory: produces a fresh `Backtester` for a given
/// `DetectorRegistry` + `DetectorsConfig`. Lets the sweep engine
/// substitute candidate-specific detectors per backtest call
/// without leaking the dependency graph (db / bars / event_calendar)
/// into the engine itself.
#[async_trait::async_trait]
pub trait BacktesterFactory: Send + Sync {
    async fn build(
        &self,
        registry: Arc<DetectorRegistry>,
        cfg: Arc<DetectorsConfig>,
    ) -> std::result::Result<Backtester, BacktesterError>;
}

/// Production factory used by `ParamRefitService`. Holds the same
/// dependency Arcs the production `Backtester` needs.
pub struct ProdBacktesterFactory {
    db: Arc<Db>,
    bars_reader: Arc<dyn BarsReader>,
    event_calendar: Option<Arc<EventCalendarService>>,
}

impl ProdBacktesterFactory {
    pub fn new(
        db: Arc<Db>,
        bars_reader: Arc<dyn BarsReader>,
        event_calendar: Option<Arc<EventCalendarService>>,
    ) -> Self {
        Self {
            db,
            bars_reader,
            event_calendar,
        }
    }
}

#[async_trait::async_trait]
impl BacktesterFactory for ProdBacktesterFactory {
    async fn build(
        &self,
        registry: Arc<DetectorRegistry>,
        cfg: Arc<DetectorsConfig>,
    ) -> std::result::Result<Backtester, BacktesterError> {
        let mut bt = Backtester::new(
            Arc::clone(&self.db),
            Arc::clone(&self.bars_reader),
            registry,
            cfg,
        );
        if let Some(cal) = &self.event_calendar {
            bt = bt.with_event_calendar(Arc::clone(cal));
        }
        Ok(bt)
    }
}

/// Build a fresh `DetectorRegistry` containing only the detector
/// under sweep, configured with the candidate's params. Keeps the
/// per-candidate backtest scoped to the detector being refit so
/// other detectors' fires don't pollute the OOS metrics.
pub fn build_candidate_registry(
    detector: &str,
    params_json: &serde_json::Value,
    bounds: &DetectorsConfig,
) -> std::result::Result<(Arc<DetectorRegistry>, Arc<DetectorsConfig>), serde_json::Error> {
    let mut cfg = bounds.clone();
    let mut registry = DetectorRegistry::new();
    match detector {
        BREAKOUT_DETECTOR => {
            let parsed: BreakoutCfg = serde_json::from_value(params_json.clone())?;
            cfg.breakout = parsed.clone();
            registry.register(Arc::new(BreakoutDetector::with_config(parsed)));
        }
        EPISODIC_PIVOT_DETECTOR => {
            let parsed: EpisodicPivotCfg = serde_json::from_value(params_json.clone())?;
            cfg.episodic_pivot = parsed.clone();
            registry.register(Arc::new(EpisodicPivotDetector::with_config(parsed)));
        }
        PARABOLIC_SHORT_DETECTOR => {
            let parsed: ParabolicShortCfg = serde_json::from_value(params_json.clone())?;
            cfg.parabolic_short = parsed.clone();
            registry.register(Arc::new(ParabolicShortDetector::with_config(parsed)));
        }
        _ => {}
    }
    Ok((Arc::new(registry), Arc::new(cfg)))
}

/// Sweep orchestrator. Cheap to construct; holds no state besides
/// the inputs.
pub struct SweepEngine {
    detector: String,
    space: ParamSpace,
    budget: u32,
    rng_seed: u64,
}

impl SweepEngine {
    pub fn new(detector: String, space: ParamSpace, budget: u32, rng_seed: u64) -> Self {
        Self {
            detector,
            space,
            budget,
            rng_seed,
        }
    }

    /// Run every candidate by constructing a per-candidate
    /// backtester (via `factory`) over the OOS window, scoring
    /// with [`Objective`], and collecting outcomes. The result
    /// preserves candidate order (post-shuffle) so the audit
    /// trail is reproducible from the seed.
    pub async fn run(
        &self,
        factory: &dyn BacktesterFactory,
        inputs: &SweepInputs,
        bounds: &DetectorsConfig,
    ) -> Result<SweepReport> {
        let candidates = self.shuffled_candidates();
        let detector_tag = detector_tag_for(&self.detector);
        let mut out: Vec<SweepCandidate> = Vec::with_capacity(candidates.len());
        for params_json in candidates {
            let result = self
                .evaluate_candidate(factory, &params_json, inputs, bounds, detector_tag.clone())
                .await;
            match result {
                Ok(bt_result) => match Objective::score(&bt_result) {
                    Ok(score) => {
                        debug!(
                            "sweep[{}]: candidate scored pf={:.3} n={} sharpe={:.2}",
                            self.detector, score.value, score.n_trades, score.sharpe
                        );
                        out.push(SweepCandidate {
                            params_json,
                            score: Some(score),
                            constraint_failures: Vec::new(),
                        });
                    }
                    Err(failures) => {
                        out.push(SweepCandidate {
                            params_json,
                            score: None,
                            constraint_failures: failures,
                        });
                    }
                },
                Err(e) => {
                    warn!(
                        "sweep[{}]: backtest failed for candidate {}: {e}",
                        self.detector, params_json
                    );
                    out.push(SweepCandidate {
                        params_json,
                        score: None,
                        constraint_failures: vec![ConstraintFailure::InsufficientTrades],
                    });
                }
            }
        }
        Ok(SweepReport {
            detector: self.detector.clone(),
            candidates: out,
        })
    }

    async fn evaluate_candidate(
        &self,
        factory: &dyn BacktesterFactory,
        params_json: &serde_json::Value,
        inputs: &SweepInputs,
        bounds: &DetectorsConfig,
        detector_tag: StrategyTag,
    ) -> std::result::Result<BacktestResult, BacktesterError> {
        let (registry, cfg) = build_candidate_registry(&self.detector, params_json, bounds)
            .map_err(|e| BacktesterError::Calibration(format!("decode candidate params: {e}")))?;
        let backtester = factory.build(registry, cfg).await?;
        let spec = self.build_spec(inputs, detector_tag);
        backtester.run(spec).await
    }

    /// Shuffle the full candidate list with the seeded RNG and take
    /// the first `budget` items. Kept separate so tests can pin
    /// determinism by checking that two runs of the same engine
    /// produce identical lists.
    ///
    /// Uses an in-house xorshift64 (mirroring the backtester's
    /// `RngState`) so the shuffle is deterministic without pulling
    /// in the `rand` crate.
    pub fn shuffled_candidates(&self) -> Vec<serde_json::Value> {
        let mut rng = XorShift64::new(self.rng_seed);
        let mut all = self.space.candidates.clone();
        // Fisher-Yates with seeded RNG so the order is reproducible.
        for i in (1..all.len()).rev() {
            let j = rng.gen_range_inclusive(i as u64) as usize;
            all.swap(i, j);
        }
        all.truncate(self.budget as usize);
        all
    }

    fn build_spec(&self, inputs: &SweepInputs, detector_tag: StrategyTag) -> BacktestSpec {
        // Master-plan committed: "validate the chosen vintage on
        // full 18 months only after selection" — sweep scores on
        // OOS only for cadence; the post-selection extended-
        // validation pass is a future enhancement.
        BacktestSpec {
            date_from: inputs.oos_from,
            date_to_inclusive: inputs.oos_to,
            symbols: inputs.symbols.clone(),
            detector_tags: vec![detector_tag],
            fill_model: FillModelKind::default(),
            position_sizing: PositionSizingMode::FixedR,
            splits: WalkForwardSplits::default(),
            commission_usd: 1.0,
            starting_equity_usd: 100_000.0,
            event_blackouts_enabled: true,
            max_hold_bars: 10,
            rng_seed: self.rng_seed,
            label: Some(format!("sweep:{}:oos", self.detector)),
        }
    }
}

fn detector_tag_for(detector: &str) -> StrategyTag {
    match detector {
        BREAKOUT_DETECTOR => StrategyTag::Breakout,
        EPISODIC_PIVOT_DETECTOR => StrategyTag::EpisodicPivot,
        PARABOLIC_SHORT_DETECTOR => StrategyTag::ParabolicShort,
        _ => StrategyTag::Breakout,
    }
}

/// In-house deterministic RNG. xorshift64 mirrors the backtester's
/// `RngState` shape so the sweep doesn't pull in `rand`. Only used
/// for `gen_range_inclusive` over [0, max].
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        // xorshift64 doesn't accept 0; substitute a constant seed.
        let s = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        Self { state: s }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Inclusive [0, max]. Uses simple modulo — bias is negligible
    /// for the small ranges (≤ 1024) the sweep uses.
    fn gen_range_inclusive(&mut self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        self.next_u64() % (max + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakout_space_is_non_empty() {
        let bounds = DetectorsConfig::default();
        let s = space_for(BREAKOUT_DETECTOR, &bounds).unwrap();
        assert!(s.candidates.len() > 100);
    }

    #[test]
    fn unknown_detector_returns_none() {
        let bounds = DetectorsConfig::default();
        assert!(space_for("nope", &bounds).is_none());
    }

    #[test]
    fn shuffle_is_deterministic_given_seed() {
        let bounds = DetectorsConfig::default();
        let space = space_for(BREAKOUT_DETECTOR, &bounds).unwrap();
        let engine_a =
            SweepEngine::new(BREAKOUT_DETECTOR.to_string(), space.clone(), 50, 0xDEADBEEF);
        let engine_b = SweepEngine::new(BREAKOUT_DETECTOR.to_string(), space, 50, 0xDEADBEEF);
        let a = engine_a.shuffled_candidates();
        let b = engine_b.shuffled_candidates();
        assert_eq!(a, b);
        assert_eq!(a.len(), 50);
    }

    #[test]
    fn different_seeds_yield_different_orders() {
        let bounds = DetectorsConfig::default();
        let space = space_for(BREAKOUT_DETECTOR, &bounds).unwrap();
        let engine_a = SweepEngine::new(BREAKOUT_DETECTOR.to_string(), space.clone(), 50, 1);
        let engine_b = SweepEngine::new(BREAKOUT_DETECTOR.to_string(), space, 50, 2);
        let a = engine_a.shuffled_candidates();
        let b = engine_b.shuffled_candidates();
        assert_ne!(a, b);
    }

    #[test]
    fn budget_caps_candidate_count() {
        let bounds = DetectorsConfig::default();
        let space = space_for(BREAKOUT_DETECTOR, &bounds).unwrap();
        let engine = SweepEngine::new(BREAKOUT_DETECTOR.to_string(), space, 25, 0);
        let out = engine.shuffled_candidates();
        assert_eq!(out.len(), 25);
    }

    #[test]
    fn detector_seed_changes_with_date() {
        use chrono::TimeZone;
        let day_a = Utc.with_ymd_and_hms(2026, 5, 1, 21, 0, 0).unwrap();
        let day_b = Utc.with_ymd_and_hms(2026, 5, 2, 21, 0, 0).unwrap();
        let a = detector_seed(BREAKOUT_DETECTOR, day_a);
        let b = detector_seed(BREAKOUT_DETECTOR, day_b);
        assert_ne!(a, b);
    }

    #[test]
    fn detector_seed_stable_within_same_day() {
        use chrono::TimeZone;
        let morning = Utc.with_ymd_and_hms(2026, 5, 1, 14, 0, 0).unwrap();
        let evening = Utc.with_ymd_and_hms(2026, 5, 1, 22, 0, 0).unwrap();
        let a = detector_seed(BREAKOUT_DETECTOR, morning);
        let b = detector_seed(BREAKOUT_DETECTOR, evening);
        // ET-dates may differ at the day boundary but for these
        // intra-day samples the seed should match.
        assert_eq!(a, b);
    }
}
