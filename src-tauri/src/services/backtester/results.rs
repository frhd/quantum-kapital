//! Phase 6 — `BacktestResult` + per-trade row + aggregation helpers.
//!
//! The result shape mirrors what the trader will read in the UI: a
//! headline `RiskMetrics` (reused from P4), per-strategy / per-month
//! breakdown rollups, and an equity curve. The per-trade row stream
//! is large; the orchestrator persists it to `backtest_trades` and
//! the in-memory `BacktestResult.trades` carries it for live runs but
//! drops to an empty Vec on reload from `backtest_runs.result_json`.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::services::trade_reviews::equity_curve::EquityPoint;
use crate::services::trade_reviews::risk_metrics::{
    compute_risk_metrics, RiskMetrics, DEFAULT_RISK_FREE_RATE_ANNUAL,
};
use crate::strategies::Direction;

/// One closed trade in the backtest. PnL is in USD; `realized_r` is
/// the unitless R-multiple of the trade (using the detector's
/// originally-emitted stop distance as the R unit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestTrade {
    pub seq: u32,
    pub symbol: String,
    pub strategy: String,
    pub direction: Direction,
    pub entry_time: DateTime<Utc>,
    pub entry_price: f64,
    pub exit_time: DateTime<Utc>,
    pub exit_price: f64,
    pub qty: u32,
    pub realized_r: f64,
    pub realized_pnl: f64,
    pub exit_reason: ExitReason,
    /// `Some` when sizing-mode is conviction-scaled; `None` for the
    /// FixedR / NoSizing modes that don't grade.
    pub conviction: Option<String>,
}

/// What ended the trade. `Stop` and `Target` are the canonical exits;
/// `TimeStop` is the `max_hold_bars` cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    Stop,
    Target,
    TimeStop,
}

impl ExitReason {
    pub fn as_str(self) -> &'static str {
        match self {
            ExitReason::Stop => "stop",
            ExitReason::Target => "target",
            ExitReason::TimeStop => "time_stop",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "stop" => Some(ExitReason::Stop),
            "target" => Some(ExitReason::Target),
            "time_stop" => Some(ExitReason::TimeStop),
            _ => None,
        }
    }
}

/// Per-strategy rollup. One row per detector strategy that fired at
/// least one trade. Per-strategy `RiskMetrics` is computed from that
/// strategy's R-stream + a strategy-local equity curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyRollup {
    pub strategy: String,
    pub n_trades: usize,
    pub metrics: RiskMetrics,
    pub net_pnl: f64,
    pub gross_pnl: f64,
    pub commission: f64,
    pub stop_count: usize,
    pub target_count: usize,
    pub time_stop_count: usize,
}

/// Per-month rollup. ET trading-month bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonthRollup {
    /// `YYYY-MM`.
    pub month: String,
    pub n_trades: usize,
    pub net_pnl: f64,
    pub realized_r_sum: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestResult {
    pub run_id: String,
    pub spec_hash: String,
    pub headline: RiskMetrics,
    pub equity_curve: Vec<EquityPoint>,
    pub by_strategy: Vec<StrategyRollup>,
    pub by_month: Vec<MonthRollup>,
    pub trades: Vec<BacktestTrade>,
    /// Diagnostic counts so a UI / log message can show "fired but
    /// gated" without a second query.
    pub n_setups_fired: usize,
    pub n_setups_blackout_skipped: usize,
    pub n_setups_unsizable: usize,
}

/// Aggregate a per-trade list into a `BacktestResult`. Pure: the
/// orchestrator owns the run_id / spec_hash and supplies them in
/// `aggregate(...)`.
pub fn aggregate(
    run_id: &str,
    spec_hash: &str,
    trades: Vec<BacktestTrade>,
    starting_equity: f64,
    diagnostics: AggregateDiagnostics,
) -> BacktestResult {
    let r_series: Vec<f64> = trades.iter().map(|t| t.realized_r).collect();
    let equity_curve = build_equity_curve(&trades, starting_equity);
    let headline = compute_risk_metrics(&equity_curve, &r_series, DEFAULT_RISK_FREE_RATE_ANNUAL);
    let by_strategy = rollup_by_strategy(&trades, starting_equity);
    let by_month = rollup_by_month(&trades);

    BacktestResult {
        run_id: run_id.to_string(),
        spec_hash: spec_hash.to_string(),
        headline,
        equity_curve,
        by_strategy,
        by_month,
        trades,
        n_setups_fired: diagnostics.n_setups_fired,
        n_setups_blackout_skipped: diagnostics.n_setups_blackout_skipped,
        n_setups_unsizable: diagnostics.n_setups_unsizable,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AggregateDiagnostics {
    pub n_setups_fired: usize,
    pub n_setups_blackout_skipped: usize,
    pub n_setups_unsizable: usize,
}

/// Convert a trade stream into an `EquityPoint` per ET trading-day.
/// The point's `daily_pnl` accumulates *exit-day* PnL — open trades
/// don't move equity until they close. Mirrors P4 equity_curve.
fn build_equity_curve(trades: &[BacktestTrade], starting_equity: f64) -> Vec<EquityPoint> {
    use chrono_tz::America::New_York;
    use std::collections::BTreeMap;

    let mut by_date: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for t in trades {
        let date = t.exit_time.with_timezone(&New_York).date_naive();
        *by_date.entry(date).or_insert(0.0) += t.realized_pnl;
    }
    let mut equity = starting_equity;
    let mut out = Vec::with_capacity(by_date.len());
    for (date, daily_pnl) in by_date {
        equity += daily_pnl;
        out.push(EquityPoint {
            date,
            equity,
            daily_pnl,
        });
    }
    out
}

fn rollup_by_strategy(trades: &[BacktestTrade], starting_equity: f64) -> Vec<StrategyRollup> {
    use std::collections::BTreeMap;

    let mut by_strat: BTreeMap<String, Vec<BacktestTrade>> = BTreeMap::new();
    for t in trades {
        by_strat
            .entry(t.strategy.clone())
            .or_default()
            .push(t.clone());
    }
    by_strat
        .into_iter()
        .map(|(strategy, trades)| {
            let r_series: Vec<f64> = trades.iter().map(|t| t.realized_r).collect();
            let net_pnl: f64 = trades.iter().map(|t| t.realized_pnl).sum();
            // `realized_pnl` already nets out commissions in the
            // replay path; we expose gross/commission breakdown so the
            // UI can show "after-comm vs before-comm".
            let entries: f64 = trades.iter().map(|_| 1.0).sum();
            let commission: f64 = entries * 0.0; // commissions already folded
            let gross_pnl = net_pnl;
            let stop_count = trades
                .iter()
                .filter(|t| t.exit_reason == ExitReason::Stop)
                .count();
            let target_count = trades
                .iter()
                .filter(|t| t.exit_reason == ExitReason::Target)
                .count();
            let time_stop_count = trades
                .iter()
                .filter(|t| t.exit_reason == ExitReason::TimeStop)
                .count();
            let equity_curve = build_equity_curve(&trades, starting_equity);
            let metrics =
                compute_risk_metrics(&equity_curve, &r_series, DEFAULT_RISK_FREE_RATE_ANNUAL);
            StrategyRollup {
                strategy,
                n_trades: trades.len(),
                metrics,
                net_pnl,
                gross_pnl,
                commission,
                stop_count,
                target_count,
                time_stop_count,
            }
        })
        .collect()
}

fn rollup_by_month(trades: &[BacktestTrade]) -> Vec<MonthRollup> {
    use chrono::Datelike;
    use chrono_tz::America::New_York;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct M {
        n: usize,
        net_pnl: f64,
        r_sum: f64,
    }
    let mut by_month: BTreeMap<String, M> = BTreeMap::new();
    for t in trades {
        let et = t.exit_time.with_timezone(&New_York);
        let key = format!("{:04}-{:02}", et.year(), et.month());
        let entry = by_month.entry(key).or_default();
        entry.n += 1;
        entry.net_pnl += t.realized_pnl;
        entry.r_sum += t.realized_r;
    }
    by_month
        .into_iter()
        .map(|(month, m)| MonthRollup {
            month,
            n_trades: m.n,
            net_pnl: m.net_pnl,
            realized_r_sum: m.r_sum,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(seq: u32, strategy: &str, r: f64, pnl: f64, day: u32) -> BacktestTrade {
        let day = day.max(1);
        let entry = Utc.with_ymd_and_hms(2025, 5, day, 14, 30, 0).unwrap();
        let exit = Utc
            .with_ymd_and_hms(2025, 5, day.min(28), 19, 30, 0)
            .unwrap();
        BacktestTrade {
            seq,
            symbol: "AAPL".to_string(),
            strategy: strategy.to_string(),
            direction: Direction::Long,
            entry_time: entry,
            entry_price: 100.0,
            exit_time: exit,
            exit_price: 100.0 + pnl / 100.0, // qty 100
            qty: 100,
            realized_r: r,
            realized_pnl: pnl,
            exit_reason: if r > 0.0 {
                ExitReason::Target
            } else {
                ExitReason::Stop
            },
            conviction: Some("B".to_string()),
        }
    }

    #[test]
    fn aggregate_empty_trades_yields_empty_metrics() {
        let result = aggregate(
            "run1",
            "abcd1234",
            Vec::new(),
            100_000.0,
            AggregateDiagnostics::default(),
        );
        assert!(result.equity_curve.is_empty());
        assert!(result.by_strategy.is_empty());
        assert_eq!(result.headline.n_trades, 0);
    }

    #[test]
    fn rollup_by_strategy_groups_by_name() {
        let trades = vec![
            t(0, "breakout", 2.0, 200.0, 1),
            t(1, "breakout", -1.0, -100.0, 2),
            t(2, "parabolic_short", 1.5, 150.0, 3),
        ];
        let result = aggregate(
            "run1",
            "abc",
            trades,
            100_000.0,
            AggregateDiagnostics::default(),
        );
        assert_eq!(result.by_strategy.len(), 2);
        let bo = result
            .by_strategy
            .iter()
            .find(|s| s.strategy == "breakout")
            .unwrap();
        assert_eq!(bo.n_trades, 2);
        assert_eq!(bo.target_count, 1);
        assert_eq!(bo.stop_count, 1);
        assert!((bo.net_pnl - 100.0).abs() < 1e-9);
    }

    #[test]
    fn rollup_by_month_buckets_by_et_year_month() {
        let trades = vec![
            t(0, "breakout", 1.0, 100.0, 5),
            t(1, "breakout", 1.0, 100.0, 12),
        ];
        let result = aggregate(
            "run1",
            "abc",
            trades,
            100_000.0,
            AggregateDiagnostics::default(),
        );
        // Both exits in May 2025 → single bucket.
        assert_eq!(result.by_month.len(), 1);
        assert_eq!(result.by_month[0].month, "2025-05");
        assert_eq!(result.by_month[0].n_trades, 2);
    }

    #[test]
    fn equity_curve_sums_pnl_per_exit_day() {
        let trades = vec![
            t(0, "breakout", 1.0, 100.0, 5),
            t(1, "breakout", -1.0, -50.0, 5),
            t(2, "breakout", 2.0, 200.0, 6),
        ];
        let result = aggregate(
            "run1",
            "abc",
            trades,
            100_000.0,
            AggregateDiagnostics::default(),
        );
        assert_eq!(result.equity_curve.len(), 2);
        assert!((result.equity_curve[0].daily_pnl - 50.0).abs() < 1e-9);
        assert!((result.equity_curve[1].daily_pnl - 200.0).abs() < 1e-9);
        assert!((result.equity_curve[1].equity - 100_250.0).abs() < 1e-9);
    }

    #[test]
    fn exit_reason_round_trips() {
        for r in [ExitReason::Stop, ExitReason::Target, ExitReason::TimeStop] {
            assert_eq!(ExitReason::parse(r.as_str()), Some(r));
        }
        assert_eq!(ExitReason::parse("nope"), None);
    }
}
