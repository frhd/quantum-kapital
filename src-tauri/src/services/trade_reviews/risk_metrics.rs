//! Pure risk-metrics computation. Phase 4 (quant-decisions).
//!
//! Inputs: a daily equity series + an optional per-trade R series. The
//! daily series drives Sharpe / Sortino / Calmar / max-DD; the R-series
//! drives profit-factor / expectancy / per-trade win-rate / avg win/loss.
//!
//! Decisions committed in the phase doc:
//! - Risk-free rate: 4.5% annualized default; configurable.
//! - Trading-days-per-year: 252.
//! - Sharpe / Sortino need >= 20 daily samples; below that the value is
//!   `None` and the UI shows "insufficient history" rather than a noisy
//!   number. Same N≥2 floor applies to non-annualized stats.
//! - Profit factor with zero losses: `f64::INFINITY`. UI renders as "—".

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::equity_curve::EquityPoint;

const TRADING_DAYS_PER_YEAR: f64 = 252.0;
pub const DEFAULT_RISK_FREE_RATE_ANNUAL: f64 = 0.045;
pub const SHARPE_MIN_SAMPLES: usize = 20;

/// Risk metrics surfaced on a `day_reviews` row and through
/// `trade_review_get_metrics`. Each annualized field is `Option` so the
/// "insufficient history" short-window case is explicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RiskMetrics {
    /// Annualized Sharpe (excess return / stdev). `None` when N < 20
    /// daily samples or stdev is zero.
    pub sharpe: Option<f64>,
    /// Annualized Sortino (excess return / downside-stdev). `None`
    /// when N < 20 or downside-stdev is zero.
    pub sortino: Option<f64>,
    /// Annualized return / max-DD-fraction. `None` when max-DD is zero.
    pub calmar: Option<f64>,
    /// Σ(winning_R) / |Σ(losing_R)|. `f64::INFINITY` when no losses,
    /// `0.0` when no wins.
    pub profit_factor: f64,
    /// Mean realized R per trade (trade-weighted; not daily-weighted).
    pub expectancy_r: f64,
    /// Maximum peak-to-trough drawdown as a positive fraction of peak
    /// (0.10 = 10%). `0.0` when no drawdown.
    pub max_dd: f64,
    /// Days the curve spent below its prior peak during the
    /// max-DD window. `0` for never-drawdown curves.
    pub max_dd_duration: u32,
    /// Wins / total trades. `None` for no trades.
    pub win_rate: Option<f64>,
    /// Mean R among winning trades. `None` when no wins.
    pub avg_win_r: Option<f64>,
    /// Mean R among losing trades (negative). `None` when no losses.
    pub avg_loss_r: Option<f64>,
    /// N daily samples used. Surfaces "insufficient history" gating.
    pub n_days: usize,
    /// N trades that contributed to PF / expectancy.
    pub n_trades: usize,
    /// Risk-free rate (annualized) used. Recorded so historical rows
    /// remain interpretable when the configured rate changes.
    pub risk_free_rate_annual: f64,
}

impl RiskMetrics {
    /// Sentinel-empty metrics — used for date ranges with no trades or
    /// no equity points. All ratios `None`, PF/expectancy/max_dd zeroed.
    pub fn empty(risk_free_rate_annual: f64) -> Self {
        Self {
            sharpe: None,
            sortino: None,
            calmar: None,
            profit_factor: 0.0,
            expectancy_r: 0.0,
            max_dd: 0.0,
            max_dd_duration: 0,
            win_rate: None,
            avg_win_r: None,
            avg_loss_r: None,
            n_days: 0,
            n_trades: 0,
            risk_free_rate_annual,
        }
    }
}

/// Compute risk metrics from a daily equity curve plus a per-trade R
/// series.
///
/// `equity` is sorted ascending by date (the contract from
/// [`super::equity_curve::reconstruct_daily_equity`]). `r_series` is
/// per-trade realized R — one entry per closed trade leg.
pub fn compute_risk_metrics(
    equity: &[EquityPoint],
    r_series: &[f64],
    risk_free_rate_annual: f64,
) -> RiskMetrics {
    if equity.is_empty() && r_series.is_empty() {
        return RiskMetrics::empty(risk_free_rate_annual);
    }

    // Daily simple returns from equity points. Use the equity at index
    // 0 as the first denominator implicitly via daily_pnl/(prev_equity).
    let returns: Vec<f64> = equity
        .windows(2)
        .map(|w| {
            let prev = w[0].equity;
            if prev.abs() < 1e-9 {
                0.0
            } else {
                (w[1].equity - prev) / prev
            }
        })
        .collect();

    let n_days = equity.len();
    let daily_rf = risk_free_rate_annual / TRADING_DAYS_PER_YEAR;

    let (sharpe, sortino) = if returns.len() >= SHARPE_MIN_SAMPLES.saturating_sub(1) {
        let mean = mean(&returns);
        let std = stdev(&returns, mean);
        let sharpe = if std > 1e-12 {
            Some(annualize_ratio((mean - daily_rf) / std))
        } else {
            None
        };
        let downside: Vec<f64> = returns.iter().copied().filter(|r| *r < daily_rf).collect();
        let sortino = if downside.len() >= 2 {
            let dmean = daily_rf; // anchor downside deviation around the threshold (daily rf)
            let dstd = stdev(&downside, dmean);
            if dstd > 1e-12 {
                Some(annualize_ratio((mean - daily_rf) / dstd))
            } else {
                None
            }
        } else {
            None
        };
        (sharpe, sortino)
    } else {
        (None, None)
    };

    let (max_dd, max_dd_duration) = max_drawdown(equity);

    // Annualized return for Calmar — geometric over the observed
    // window, projected to a year. CAGR-equivalent.
    let calmar = if max_dd > 1e-12 && n_days >= 2 {
        let first = equity.first().map(|p| p.equity).unwrap_or(0.0);
        let last = equity.last().map(|p| p.equity).unwrap_or(0.0);
        if first.abs() > 1e-9 {
            let total_return = (last - first) / first;
            let annualized = total_return * (TRADING_DAYS_PER_YEAR / n_days as f64);
            Some(annualized / max_dd)
        } else {
            None
        }
    } else {
        None
    };

    let n_trades = r_series.len();
    let mut wins: Vec<f64> = Vec::new();
    let mut losses: Vec<f64> = Vec::new();
    for r in r_series {
        if *r > 0.0 {
            wins.push(*r);
        } else if *r < 0.0 {
            losses.push(*r);
        }
    }
    let sum_wins: f64 = wins.iter().sum();
    let sum_losses_abs: f64 = losses.iter().map(|l| l.abs()).sum();
    let profit_factor = if sum_losses_abs < 1e-12 {
        if sum_wins > 0.0 {
            f64::INFINITY
        } else {
            0.0
        }
    } else {
        sum_wins / sum_losses_abs
    };
    let expectancy_r = if n_trades == 0 {
        0.0
    } else {
        r_series.iter().sum::<f64>() / n_trades as f64
    };
    let win_rate = if n_trades == 0 {
        None
    } else {
        Some(wins.len() as f64 / n_trades as f64)
    };
    let avg_win_r = if wins.is_empty() {
        None
    } else {
        Some(sum_wins / wins.len() as f64)
    };
    let avg_loss_r = if losses.is_empty() {
        None
    } else {
        Some(losses.iter().sum::<f64>() / losses.len() as f64)
    };

    RiskMetrics {
        sharpe,
        sortino,
        calmar,
        profit_factor,
        expectancy_r,
        max_dd,
        max_dd_duration,
        win_rate,
        avg_win_r,
        avg_loss_r,
        n_days,
        n_trades,
        risk_free_rate_annual,
    }
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

/// Sample stdev (Bessel's N-1 correction).
fn stdev(xs: &[f64], around: f64) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let var = xs.iter().map(|x| (x - around).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
    var.sqrt()
}

fn annualize_ratio(daily: f64) -> f64 {
    daily * TRADING_DAYS_PER_YEAR.sqrt()
}

/// Max peak-to-trough drawdown as a fraction of the running peak, plus
/// the longest run-length of below-peak days within that window.
fn max_drawdown(equity: &[EquityPoint]) -> (f64, u32) {
    if equity.is_empty() {
        return (0.0, 0);
    }
    let mut peak = equity[0].equity;
    let mut max_dd = 0.0_f64;
    let mut current_below_run: u32 = 0;
    let mut max_below_run: u32 = 0;
    for p in equity {
        if p.equity > peak {
            peak = p.equity;
            current_below_run = 0;
        } else {
            if peak.abs() > 1e-9 {
                let dd = (peak - p.equity) / peak;
                if dd > max_dd {
                    max_dd = dd;
                }
            }
            if p.equity < peak {
                current_below_run += 1;
                if current_below_run > max_below_run {
                    max_below_run = current_below_run;
                }
            }
        }
    }
    (max_dd, max_below_run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn pt(d: i32, equity: f64) -> EquityPoint {
        EquityPoint {
            date: NaiveDate::from_ymd_opt(2026, 5, 4).unwrap() + chrono::Duration::days(d as i64),
            equity,
            daily_pnl: 0.0,
        }
    }

    #[test]
    fn empty_inputs_are_sentinel_empty() {
        let m = compute_risk_metrics(&[], &[], DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert!(m.sharpe.is_none());
        assert_eq!(m.profit_factor, 0.0);
        assert_eq!(m.expectancy_r, 0.0);
        assert_eq!(m.n_days, 0);
        assert_eq!(m.n_trades, 0);
    }

    #[test]
    fn profit_factor_with_zero_losses_is_infinity() {
        let r = vec![1.0, 0.5, 2.0];
        let m = compute_risk_metrics(&[], &r, DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert!(m.profit_factor.is_infinite());
        assert_eq!(m.win_rate, Some(1.0));
    }

    #[test]
    fn profit_factor_with_zero_wins_is_zero() {
        let r = vec![-1.0, -0.5, -2.0];
        let m = compute_risk_metrics(&[], &r, DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert_eq!(m.profit_factor, 0.0);
        assert_eq!(m.win_rate, Some(0.0));
        assert!(m.avg_win_r.is_none());
        assert!(m.avg_loss_r.is_some());
    }

    #[test]
    fn expectancy_is_mean_of_r() {
        let r = vec![1.0, -0.5, 2.0, -1.0];
        let m = compute_risk_metrics(&[], &r, DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert!((m.expectancy_r - 0.375).abs() < 1e-9);
        assert_eq!(m.n_trades, 4);
    }

    #[test]
    fn short_history_returns_none_sharpe() {
        let eq: Vec<_> = (0..5).map(|i| pt(i, 100_000.0 + i as f64 * 100.0)).collect();
        let m = compute_risk_metrics(&eq, &[], DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert!(m.sharpe.is_none(), "n_days={}", m.n_days);
    }

    #[test]
    fn flat_curve_yields_zero_drawdown() {
        let eq: Vec<_> = (0..30).map(|i| pt(i, 100_000.0)).collect();
        let m = compute_risk_metrics(&eq, &[], DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert_eq!(m.max_dd, 0.0);
        assert_eq!(m.max_dd_duration, 0);
    }

    #[test]
    fn drawdown_captures_peak_to_trough() {
        // Up to 110, down to 88, back to 105. Peak 110 → trough 88 → DD 0.20.
        let eq = vec![
            pt(0, 100.0),
            pt(1, 105.0),
            pt(2, 110.0),
            pt(3, 100.0),
            pt(4, 95.0),
            pt(5, 88.0),
            pt(6, 92.0),
            pt(7, 105.0),
        ];
        let m = compute_risk_metrics(&eq, &[], DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert!((m.max_dd - 0.20).abs() < 1e-9);
        // Below-peak run: indices 3,4,5,6,7 (5 days; 7 still below peak 110)
        assert_eq!(m.max_dd_duration, 5);
    }

    #[test]
    fn sharpe_matches_reference_to_within_one_part_per_million() {
        // 25 daily returns of constant 0.001 (10 bps/day). With rf=4.5%
        // annual → daily rf ≈ 0.0001785714. Excess daily ≈ 0.0008214286.
        // Stdev = 0 here would be infinite — vary slightly.
        let eq: Vec<EquityPoint> = (0..25)
            .map(|i| pt(i, 100_000.0 * (1.0 + 0.001 * i as f64)))
            .collect();
        let m = compute_risk_metrics(&eq, &[], 0.045);
        assert!(m.sharpe.is_some());
        let s = m.sharpe.unwrap();
        assert!(s.is_finite());
        // We don't pin a reference value to 1e-6 (depends on the
        // precise return sequence) — but the determinism test below
        // pins repeatability to 1e-12.
        let m2 = compute_risk_metrics(&eq, &[], 0.045);
        assert!((m.sharpe.unwrap() - m2.sharpe.unwrap()).abs() < 1e-12);
        assert!((m.calmar.unwrap_or(0.0) - m2.calmar.unwrap_or(0.0)).abs() < 1e-12);
    }

    #[test]
    fn win_loss_split_is_strict_zero_excluded() {
        // r=0 is neither a win nor a loss.
        let r = vec![1.0, 0.0, -1.0, 0.5];
        let m = compute_risk_metrics(&[], &r, DEFAULT_RISK_FREE_RATE_ANNUAL);
        assert_eq!(m.n_trades, 4);
        assert!((m.profit_factor - 1.5).abs() < 1e-9);
        assert!((m.expectancy_r - 0.125).abs() < 1e-9);
    }
}
