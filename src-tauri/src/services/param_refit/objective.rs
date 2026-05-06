//! Phase 10 — objective scoring for sweep candidates.
//!
//! Master-plan committed: the objective is OOS profit factor, gated
//! by hard constraints (`min 30 OOS trades`, `OOS Sharpe ≥ 0.5`,
//! `expectancy in R ≥ 0.1`). A candidate that fails any constraint
//! is ineligible to unseat the active vintage regardless of how
//! large its raw profit factor is. The lock-on-improvement check
//! (`new ≥ baseline × 1.10`) is applied AFTER scoring in
//! [`super::ParamRefitService::run_one`]; this module only handles
//! per-candidate scoring.
//!
//! `ObjectiveScore::value` is the comparable scalar (profit factor),
//! exposed alongside the constituent metrics so the audit trail
//! (`attempted_configs_json`) can show *why* a candidate scored
//! what it did. `value` collapses to `0.0` when the candidate has
//! zero gross losses (PF would otherwise be `+∞`); the constraint
//! guard catches the degenerate case earlier.

use serde::{Deserialize, Serialize};

use crate::services::backtester::BacktestResult;

/// Master-plan committed hard constraints for the OOS evaluation.
/// Vintages that fail any of these are rejected by the sweep
/// regardless of their objective value.
pub const MIN_OOS_TRADES: usize = 30;
pub const MIN_OOS_SHARPE: f64 = 0.5;
pub const MIN_OOS_EXPECTANCY_R: f64 = 0.1;

/// Constraint guard outcome. The sweep audit array surfaces these
/// as strings so the eval panel can show which guard tripped on
/// rejected configs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintFailure {
    InsufficientTrades,
    SharpeBelowFloor,
    ExpectancyBelowFloor,
    /// PF was infinite (zero gross losses) — treated as a failure
    /// so the lock guard doesn't accept a "perfect" small-N run.
    DegeneratePf,
}

impl ConstraintFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            ConstraintFailure::InsufficientTrades => "insufficient_trades",
            ConstraintFailure::SharpeBelowFloor => "sharpe_below_floor",
            ConstraintFailure::ExpectancyBelowFloor => "expectancy_below_floor",
            ConstraintFailure::DegeneratePf => "degenerate_pf",
        }
    }
}

/// Score for one passing candidate. The lock-on-improvement check
/// reads `value`; the audit panel reads the constituent metrics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ObjectiveScore {
    /// Comparable scalar — profit factor, capped (no infinity).
    pub value: f64,
    pub n_trades: usize,
    pub sharpe: f64,
    pub expectancy_r: f64,
}

/// Pure scorer: input is a backtest result over the OOS window;
/// output is `Ok(ObjectiveScore)` for passing configs and
/// `Err(Vec<ConstraintFailure>)` for rejected ones. The vector is
/// cumulative — a candidate that fails on trade count AND Sharpe
/// reports both, so the audit shows the full picture.
pub struct Objective;

impl Objective {
    pub fn score(
        result: &BacktestResult,
    ) -> std::result::Result<ObjectiveScore, Vec<ConstraintFailure>> {
        let mut failures = Vec::new();
        let n_trades = result.headline.n_trades;
        let pf = result.headline.profit_factor;
        let sharpe = result.headline.sharpe.unwrap_or(0.0);
        let expectancy_r = result.headline.expectancy_r;

        if n_trades < MIN_OOS_TRADES {
            failures.push(ConstraintFailure::InsufficientTrades);
        }
        if sharpe < MIN_OOS_SHARPE {
            failures.push(ConstraintFailure::SharpeBelowFloor);
        }
        if expectancy_r < MIN_OOS_EXPECTANCY_R {
            failures.push(ConstraintFailure::ExpectancyBelowFloor);
        }
        if !pf.is_finite() {
            failures.push(ConstraintFailure::DegeneratePf);
        }
        if !failures.is_empty() {
            return Err(failures);
        }
        Ok(ObjectiveScore {
            value: pf,
            n_trades,
            sharpe,
            expectancy_r,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::trade_reviews::risk_metrics::RiskMetrics;

    fn synthetic_result(n_trades: usize, pf: f64, sharpe: f64, expectancy: f64) -> BacktestResult {
        let headline = RiskMetrics {
            sharpe: Some(sharpe),
            sortino: None,
            calmar: None,
            profit_factor: pf,
            expectancy_r: expectancy,
            max_dd: 0.0,
            max_dd_duration: 0,
            win_rate: Some(0.6),
            avg_win_r: Some(1.5),
            avg_loss_r: Some(-1.0),
            n_days: n_trades,
            n_trades,
            risk_free_rate_annual: 0.045,
        };
        BacktestResult {
            run_id: "rid".into(),
            spec_hash: "h".into(),
            headline,
            equity_curve: Vec::new(),
            by_strategy: Vec::new(),
            by_month: Vec::new(),
            trades: Vec::new(),
            n_setups_fired: 0,
            n_setups_blackout_skipped: 0,
            n_setups_unsizable: 0,
        }
    }

    #[test]
    fn passing_candidate_returns_pf_value() {
        let r = synthetic_result(40, 1.5, 0.8, 0.2);
        let score = Objective::score(&r).expect("should pass");
        assert_eq!(score.n_trades, 40);
        assert!((score.value - 1.5).abs() < 1e-9);
    }

    #[test]
    fn insufficient_trades_rejected_even_with_high_pf() {
        let r = synthetic_result(10, 5.0, 2.0, 1.0);
        let failures = Objective::score(&r).unwrap_err();
        assert!(failures.contains(&ConstraintFailure::InsufficientTrades));
    }

    #[test]
    fn sharpe_below_floor_rejected() {
        let r = synthetic_result(50, 1.5, 0.3, 0.2);
        let failures = Objective::score(&r).unwrap_err();
        assert!(failures.contains(&ConstraintFailure::SharpeBelowFloor));
    }

    #[test]
    fn expectancy_below_floor_rejected() {
        let r = synthetic_result(50, 1.5, 0.8, 0.05);
        let failures = Objective::score(&r).unwrap_err();
        assert!(failures.contains(&ConstraintFailure::ExpectancyBelowFloor));
    }

    #[test]
    fn degenerate_pf_rejected() {
        let r = synthetic_result(50, f64::INFINITY, 1.0, 0.5);
        let failures = Objective::score(&r).unwrap_err();
        assert!(failures.contains(&ConstraintFailure::DegeneratePf));
    }

    #[test]
    fn multiple_failures_accumulate() {
        let r = synthetic_result(5, 1.5, 0.2, 0.05);
        let failures = Objective::score(&r).unwrap_err();
        assert_eq!(failures.len(), 3);
    }
}
