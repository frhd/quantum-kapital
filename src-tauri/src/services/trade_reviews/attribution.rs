//! Per-strategy (= detector class) attribution rollup. Phase 4.
//!
//! Pure aggregator: takes Phase-2-linked legs and groups them by
//! `strategy`. Computes per-strategy realized PnL, average R, win
//! rate, profit factor, and a 30-day rolling Sharpe (passes through to
//! `risk_metrics::compute_risk_metrics` on the per-strategy daily PnL
//! series).
//!
//! Granularity: detector class only (committed in the phase doc; finer
//! grain after P10 walk-forward refit).

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::equity_curve::EquityPoint;
use super::risk_metrics::{compute_risk_metrics, DEFAULT_RISK_FREE_RATE_ANNUAL};
use crate::services::trade_legs::TradeLeg;

/// One strategy's roll-up. The Sharpe field is annualized over the
/// per-strategy daily PnL series of the date-range under review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StrategyRollup {
    /// Detector class (`"breakout"`, `"parabolic_short"`,
    /// `"episodic_pivot"`, …) — or `"unattributed"` for legs whose
    /// opening fill did not carry a strategy.
    pub strategy: String,
    pub n_trades: usize,
    pub realized_pnl: f64,
    /// Mean realized R per trade. `None` when N=0 OR when no leg
    /// could be reduced to an R (missing dollar-risk on the setup).
    pub avg_r: Option<f64>,
    /// Wins / total trades. `None` for empty bucket.
    pub win_rate: Option<f64>,
    /// Σ winners / |Σ losers|. `f64::INFINITY` for no-loss buckets.
    pub profit_factor: f64,
    /// Annualized Sharpe over the per-strategy daily PnL series.
    /// `None` when N < 20 daily samples.
    pub sharpe_30d: Option<f64>,
}

/// Per-leg-with-R input. Built by the generator from
/// `(TradeLeg, Setup row's risk_engine grade)`. R is `None` when the
/// leg has no `setup_id` or the setup has NULL `dollar_risk_cents`
/// (pre-P1 setups).
#[derive(Debug, Clone)]
pub struct LegWithR<'a> {
    pub leg: &'a TradeLeg,
    pub realized_r: Option<f64>,
}

/// Group by strategy and compute per-strategy stats. The returned
/// vector is sorted by strategy name; the unattributed bucket appears
/// as `"unattributed"`.
pub fn rollup_by_strategy(legs: &[LegWithR<'_>]) -> Vec<StrategyRollup> {
    let mut by_strategy: std::collections::BTreeMap<String, Vec<&LegWithR>> = Default::default();
    for l in legs {
        let strategy = l
            .leg
            .strategy
            .clone()
            .unwrap_or_else(|| "unattributed".into());
        by_strategy.entry(strategy).or_default().push(l);
    }
    by_strategy
        .into_iter()
        .map(|(strategy, group)| {
            let n_trades = group.len();
            let realized_pnl: f64 = group.iter().map(|g| g.leg.net_pnl).sum();
            let r_values: Vec<f64> = group.iter().filter_map(|g| g.realized_r).collect();
            let avg_r = if r_values.is_empty() {
                None
            } else {
                Some(r_values.iter().sum::<f64>() / r_values.len() as f64)
            };
            let n_wins = group.iter().filter(|g| g.leg.net_pnl > 0.0).count();
            let win_rate = if n_trades == 0 {
                None
            } else {
                Some(n_wins as f64 / n_trades as f64)
            };
            let sum_wins: f64 = group
                .iter()
                .filter(|g| g.leg.net_pnl > 0.0)
                .map(|g| g.leg.net_pnl)
                .sum();
            let sum_losses_abs: f64 = group
                .iter()
                .filter(|g| g.leg.net_pnl < 0.0)
                .map(|g| g.leg.net_pnl.abs())
                .sum();
            let profit_factor = if sum_losses_abs < 1e-12 {
                if sum_wins > 0.0 {
                    f64::INFINITY
                } else {
                    0.0
                }
            } else {
                sum_wins / sum_losses_abs
            };

            // Build a per-strategy daily equity series for Sharpe.
            let mut by_date: std::collections::BTreeMap<NaiveDate, f64> = Default::default();
            for g in &group {
                let close = g
                    .leg
                    .closed_at
                    .unwrap_or(g.leg.opened_at)
                    .date_naive();
                *by_date.entry(close).or_insert(0.0) += g.leg.net_pnl;
            }
            let mut equity = 0.0;
            let curve: Vec<EquityPoint> = by_date
                .into_iter()
                .map(|(date, daily_pnl)| {
                    equity += daily_pnl;
                    EquityPoint {
                        date,
                        equity,
                        daily_pnl,
                    }
                })
                .collect();
            let metrics = compute_risk_metrics(&curve, &r_values, DEFAULT_RISK_FREE_RATE_ANNUAL);
            StrategyRollup {
                strategy,
                n_trades,
                realized_pnl,
                avg_r,
                win_rate,
                profit_factor,
                sharpe_30d: metrics.sharpe,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::trade_legs::{LegTag, TradeLeg};
    use chrono::{TimeZone, Utc};

    fn leg(strategy: Option<&str>, net_pnl: f64, day_offset: i64) -> TradeLeg {
        let opened = Utc.with_ymd_and_hms(2026, 5, 4, 14, 0, 0).unwrap()
            + chrono::Duration::days(day_offset);
        TradeLeg {
            leg_id: format!("L{day_offset}-{net_pnl}"),
            account: "U1".into(),
            symbol: "X".into(),
            contract_type: "STK".into(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            opened_at: opened,
            closed_at: Some(opened + chrono::Duration::minutes(60)),
            buy_qty: 100.0,
            avg_buy_price: 1.0,
            sell_qty: 100.0,
            avg_sell_price: 1.0,
            gross_pnl: net_pnl,
            commission_total: 0.0,
            net_pnl,
            hold_minutes: Some(60),
            source_exec_ids: vec![],
            tags: vec![LegTag::RoundTrip],
            strategy: strategy.map(|s| s.into()),
            setup_id: Some(1),
        }
    }

    #[test]
    fn unattributed_bucket_for_legs_without_strategy() {
        let l1 = leg(None, 100.0, 0);
        let l2 = leg(None, -50.0, 0);
        let inputs = vec![
            LegWithR {
                leg: &l1,
                realized_r: Some(2.0),
            },
            LegWithR {
                leg: &l2,
                realized_r: Some(-1.0),
            },
        ];
        let r = rollup_by_strategy(&inputs);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].strategy, "unattributed");
        assert_eq!(r[0].n_trades, 2);
        assert!((r[0].realized_pnl - 50.0).abs() < 1e-9);
        assert!((r[0].avg_r.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(r[0].win_rate, Some(0.5));
    }

    #[test]
    fn group_by_strategy_yields_one_row_per_class() {
        let l1 = leg(Some("breakout"), 100.0, 0);
        let l2 = leg(Some("breakout"), 50.0, 1);
        let l3 = leg(Some("parabolic_short"), -75.0, 0);
        let inputs = vec![
            LegWithR {
                leg: &l1,
                realized_r: Some(2.0),
            },
            LegWithR {
                leg: &l2,
                realized_r: Some(1.0),
            },
            LegWithR {
                leg: &l3,
                realized_r: Some(-1.5),
            },
        ];
        let r = rollup_by_strategy(&inputs);
        assert_eq!(r.len(), 2);
        // BTreeMap → alphabetical
        assert_eq!(r[0].strategy, "breakout");
        assert_eq!(r[1].strategy, "parabolic_short");
        assert_eq!(r[0].n_trades, 2);
        assert!((r[0].realized_pnl - 150.0).abs() < 1e-9);
        assert_eq!(r[0].win_rate, Some(1.0));
        assert!(r[0].profit_factor.is_infinite());
    }

    #[test]
    fn no_r_means_avg_r_is_none() {
        let l = leg(Some("episodic_pivot"), 100.0, 0);
        let r = rollup_by_strategy(&[LegWithR {
            leg: &l,
            realized_r: None,
        }]);
        assert!(r[0].avg_r.is_none());
    }
}
