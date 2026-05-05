//! Phase 6 — walk-forward split helpers.
//!
//! The replay loop runs detectors over the *full* date range; the
//! walk-forward layer is purely a *post-processing* filter that
//! restricts a result's per-trade list to the OOS portion of each
//! split. Master-plan committed: 12-month train, 3-month OOS,
//! 1-month roll.
//!
//! Train windows aren't refit in P6 (param refit is P10) — they are
//! observed only insofar as the trade count gates "insufficient
//! sample" warnings on the OOS rollup. Even so, surfacing the train/
//! OOS split now means the same `walk_forward_partition` helper can
//! be re-used by P10's calibrator without renaming.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use super::results::BacktestTrade;
use super::spec::WalkForwardSplits;

/// One split in the walk-forward sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    pub train_start: NaiveDate,
    pub oos_start: NaiveDate,
    /// Inclusive. The next split's `oos_start` is the day after this.
    pub oos_end_inclusive: NaiveDate,
}

/// Generate the rolling sequence of (train, OOS) splits over the
/// `[date_from, date_to_inclusive]` window. Each split's train is
/// `train_months` long ending the day before `oos_start`; OOS is
/// `oos_months` long; roll forward by `roll_months`. Splits whose
/// `oos_end_inclusive` would exceed `date_to_inclusive` are clamped
/// — the final split may have a shorter OOS than the others.
///
/// Returns an empty vec when the range is too small for even one
/// split (i.e., the train window can't fit before `date_from +
/// train_months`).
pub fn build_splits(
    date_from: NaiveDate,
    date_to_inclusive: NaiveDate,
    cfg: WalkForwardSplits,
) -> Vec<Split> {
    if cfg.oos_months == 0 || cfg.train_months == 0 || date_from > date_to_inclusive {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut oos_start = add_months(date_from, cfg.train_months);
    let roll = cfg.roll_months.max(1);
    while oos_start <= date_to_inclusive {
        let oos_end = add_months(oos_start, cfg.oos_months);
        // OOS window is half-open in months math; convert to
        // inclusive-day by stepping back one day.
        let oos_end_inclusive = oos_end.pred_opt().unwrap_or(oos_end).min(date_to_inclusive);
        let train_start = sub_months(oos_start, cfg.train_months);
        out.push(Split {
            train_start,
            oos_start,
            oos_end_inclusive,
        });
        if oos_end_inclusive >= date_to_inclusive {
            break;
        }
        oos_start = add_months(oos_start, roll);
    }
    out
}

/// Filter `trades` to those whose `entry_time` falls inside any split's
/// OOS window. A trade is OOS iff its entry date is in
/// `[oos_start, oos_end_inclusive]` for at least one split.
pub fn filter_oos<'a>(trades: &'a [BacktestTrade], splits: &[Split]) -> Vec<&'a BacktestTrade> {
    use chrono_tz::America::New_York;
    trades
        .iter()
        .filter(|t| {
            let date = t.entry_time.with_timezone(&New_York).date_naive();
            splits
                .iter()
                .any(|s| date >= s.oos_start && date <= s.oos_end_inclusive)
        })
        .collect()
}

/// Add `n` calendar months to `date`, clamping to the last day of the
/// resulting month. `chrono` doesn't ship a months-arithmetic helper
/// so we roll our own — care with month overflow + Feb-29 → Feb-28
/// roll-down.
fn add_months(date: NaiveDate, n: u32) -> NaiveDate {
    let total = date.month0() as i64 + n as i64;
    let years = total.div_euclid(12);
    let month0 = total.rem_euclid(12) as u32;
    let new_year = date.year() + years as i32;
    let new_month = month0 + 1;
    let last_day_of_month = days_in_month(new_year, new_month);
    let new_day = date.day().min(last_day_of_month);
    NaiveDate::from_ymd_opt(new_year, new_month, new_day).expect("valid clamped date")
}

fn sub_months(date: NaiveDate, n: u32) -> NaiveDate {
    let total = date.month0() as i64 - n as i64;
    let years = total.div_euclid(12);
    let month0 = total.rem_euclid(12) as u32;
    let new_year = date.year() + years as i32;
    let new_month = month0 + 1;
    let last_day_of_month = days_in_month(new_year, new_month);
    let new_day = date.day().min(last_day_of_month);
    NaiveDate::from_ymd_opt(new_year, new_month, new_day).expect("valid clamped date")
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => unreachable!("month out of range"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;

    use crate::services::backtester::results::ExitReason;
    use crate::strategies::Direction;

    #[test]
    fn add_months_simple() {
        let d = NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
        assert_eq!(
            add_months(d, 3),
            NaiveDate::from_ymd_opt(2025, 4, 15).unwrap()
        );
        assert_eq!(
            add_months(d, 12),
            NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
        );
    }

    #[test]
    fn add_months_clamps_end_of_month() {
        let d = NaiveDate::from_ymd_opt(2025, 1, 31).unwrap();
        // Jan 31 + 1mo → Feb 28 (2025 is non-leap)
        assert_eq!(
            add_months(d, 1),
            NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()
        );
    }

    #[test]
    fn build_splits_default_config_yields_rolling_oos() {
        let from = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
        let splits = build_splits(
            from,
            to,
            WalkForwardSplits {
                train_months: 12,
                oos_months: 3,
                roll_months: 1,
            },
        );
        // First OOS starts 2025-01-01.
        assert_eq!(
            splits[0].oos_start,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()
        );
        // Train start = 2024-01-01.
        assert_eq!(
            splits[0].train_start,
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
        );
        // OOS end inclusive = 2025-03-31.
        assert_eq!(
            splits[0].oos_end_inclusive,
            NaiveDate::from_ymd_opt(2025, 3, 31).unwrap()
        );
        // Second split rolls 1 month forward.
        assert_eq!(
            splits[1].oos_start,
            NaiveDate::from_ymd_opt(2025, 2, 1).unwrap()
        );
        // Final split's OOS clamped to date_to_inclusive.
        let last = splits.last().unwrap();
        assert!(last.oos_end_inclusive <= to);
    }

    #[test]
    fn build_splits_returns_empty_when_too_short() {
        let from = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(); // 6 months
        let splits = build_splits(
            from,
            to,
            WalkForwardSplits {
                train_months: 12,
                oos_months: 3,
                roll_months: 1,
            },
        );
        assert!(splits.is_empty());
    }

    #[test]
    fn filter_oos_keeps_only_in_window_trades() {
        let splits = vec![Split {
            train_start: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            oos_start: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            oos_end_inclusive: NaiveDate::from_ymd_opt(2025, 3, 31).unwrap(),
        }];
        let trades = vec![
            mk_trade(0, "AAPL", "breakout", 2024, 12, 15),
            mk_trade(1, "AAPL", "breakout", 2025, 2, 1),
            mk_trade(2, "AAPL", "breakout", 2025, 5, 1),
        ];
        let oos = filter_oos(&trades, &splits);
        assert_eq!(oos.len(), 1);
        assert_eq!(oos[0].seq, 1);
    }

    fn mk_trade(seq: u32, sym: &str, strat: &str, y: i32, m: u32, d: u32) -> BacktestTrade {
        let entry = Utc.with_ymd_and_hms(y, m, d, 14, 30, 0).unwrap();
        BacktestTrade {
            seq,
            symbol: sym.to_string(),
            strategy: strat.to_string(),
            direction: Direction::Long,
            entry_time: entry,
            entry_price: 100.0,
            exit_time: entry,
            exit_price: 102.0,
            qty: 100,
            realized_r: 1.0,
            realized_pnl: 200.0,
            exit_reason: ExitReason::Target,
            conviction: Some("B".to_string()),
        }
    }
}
