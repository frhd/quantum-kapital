//! Pure daily-equity-curve reconstruction from a slice of executions.
//!
//! Phase 4 (quant-decisions). Walks an ordered, contiguous date series
//! and accumulates the day's `realized_pnl - commission` across all
//! fills that close on that date. Pre-P4 had no equity curve; this is
//! the input to `risk_metrics::compute_risk_metrics`.
//!
//! Deposits, withdrawals, dividends, and non-trade fees are NOT
//! reconstructed here — `account_summary` deltas would be needed to
//! catch them, and that service does not exist yet (logged as a Phase
//! 4 gotcha in QUESTIONS.md). The curve we produce is the
//! trade-flow-only equity series; a "reconciliation_warning" rider on
//! the wider equity series is left to the phase that adds NLV
//! snapshots.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::America::New_York;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::mcp::tools::executions::ExecutionRow;

/// One point on the daily equity curve. `equity` is cumulative
/// trade-flow equity assuming a `starting_equity` baseline; `daily_pnl`
/// is the sum of realized - commission for that ET trading day. Days
/// with no fills appear iff the caller asks the curve to be filled in
/// (see `reconstruct_daily_equity_with_calendar`); the bare reconstruct
/// path emits one entry per ET-date that had any activity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EquityPoint {
    pub date: NaiveDate,
    pub equity: f64,
    pub daily_pnl: f64,
}

/// Reconstruct the daily equity curve from `executions`. Each fill's
/// `realized_pnl - commission` lands on the ET trading-day boundary of
/// `exec_time`. Fills with NULL realized_pnl are treated as 0 (open
/// legs do not move realized equity). The output is sorted ascending
/// by date and contains one entry per date that had any fill.
///
/// `starting_equity` is the cumulative-equity baseline before the
/// first day. Caller-supplied so the same curve can be rendered as
/// "intraday since 09:30" (baseline = T-1 close NLV) or "since
/// inception" (baseline = 0).
pub fn reconstruct_daily_equity(
    executions: &[ExecutionRow],
    starting_equity: f64,
) -> Vec<EquityPoint> {
    let mut by_date: std::collections::BTreeMap<NaiveDate, f64> = Default::default();
    for fill in executions {
        let date = et_date_of(fill.time);
        let pnl = fill.realized_pnl.unwrap_or(0.0) - fill.commission.unwrap_or(0.0);
        *by_date.entry(date).or_insert(0.0) += pnl;
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

/// Convert a UTC fill timestamp to the ET trading-day it belongs to.
/// DST-correct via `chrono_tz::America::New_York`.
fn et_date_of(t: DateTime<Utc>) -> NaiveDate {
    t.with_timezone(&New_York).date_naive()
}

/// Convenience for tests / call sites that already hold an ET-local
/// date and want to emit a synthetic point.
#[allow(dead_code)]
pub fn et_date_naive(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
}

#[cfg(test)]
fn et_dt(date: NaiveDate, h: u32, m: u32) -> DateTime<Utc> {
    use chrono::TimeZone;
    let naive = date.and_hms_opt(h, m, 0).unwrap();
    New_York
        .from_local_datetime(&naive)
        .single()
        .expect("ET unambiguous")
        .with_timezone(&Utc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::ExecutionSide;

    fn fill(
        date: NaiveDate,
        hour: u32,
        realized: Option<f64>,
        commission: Option<f64>,
    ) -> ExecutionRow {
        ExecutionRow {
            exec_id: format!("{date}-{hour}"),
            account: "U1".into(),
            symbol: "AAPL".into(),
            contract_type: "STK".into(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            side: ExecutionSide::Sold,
            qty: 100.0,
            avg_price: 200.0,
            currency: Some("USD".into()),
            time: et_dt(date, hour, 0),
            order_id: 1,
            commission,
            realized_pnl: realized,
            commission_currency: Some("USD".into()),
            setup_id: None,
            strategy: None,
            slippage_bps: None,
        }
    }

    #[test]
    fn empty_input_returns_empty_curve() {
        let pts = reconstruct_daily_equity(&[], 100_000.0);
        assert!(pts.is_empty());
    }

    #[test]
    fn single_day_two_fills_sums_into_one_point() {
        let d = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let fills = vec![
            fill(d, 10, Some(150.0), Some(1.0)),
            fill(d, 14, Some(-50.0), Some(1.0)),
        ];
        let pts = reconstruct_daily_equity(&fills, 100_000.0);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].date, d);
        // 150 - 1 + (-50) - 1 = 98
        assert!((pts[0].daily_pnl - 98.0).abs() < 1e-9);
        assert!((pts[0].equity - 100_098.0).abs() < 1e-9);
    }

    #[test]
    fn three_days_compound_into_running_equity() {
        let d1 = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
        let d3 = NaiveDate::from_ymd_opt(2026, 5, 6).unwrap();
        let fills = vec![
            fill(d1, 10, Some(200.0), Some(1.0)),
            fill(d2, 11, Some(-100.0), Some(1.0)),
            fill(d3, 12, Some(50.0), Some(0.5)),
        ];
        let pts = reconstruct_daily_equity(&fills, 100_000.0);
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0].date, d1);
        assert!((pts[0].equity - 100_199.0).abs() < 1e-9);
        assert!((pts[1].equity - 100_098.0).abs() < 1e-9);
        assert!((pts[2].equity - 100_147.5).abs() < 1e-9);
    }

    #[test]
    fn missing_realized_pnl_is_treated_as_zero() {
        let d = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        // Open leg with no realized; commission only.
        let fills = vec![fill(d, 10, None, Some(1.0))];
        let pts = reconstruct_daily_equity(&fills, 0.0);
        assert_eq!(pts.len(), 1);
        assert!((pts[0].daily_pnl - -1.0).abs() < 1e-9);
    }

    #[test]
    fn et_date_groups_late_session_correctly() {
        // 2026-05-04 18:00 ET (after-hours) groups with 2026-05-04.
        let d = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let fills = vec![fill(d, 18, Some(10.0), Some(0.0))];
        let pts = reconstruct_daily_equity(&fills, 0.0);
        assert_eq!(pts[0].date, d);
    }
}
