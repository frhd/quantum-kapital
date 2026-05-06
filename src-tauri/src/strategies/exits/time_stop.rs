//! Phase 7 — time-stop spec + bar-counting helpers.
//!
//! "Close the position at N trading days elapsed if neither target
//! nor stop has hit." The math anchors on the parent fill timestamp
//! and the calendar in `utils/market_calendar/`, so weekends and
//! holidays don't count toward the horizon — same calendar the
//! tracker scheduler and earnings blackout use.
//!
//! Pure functions only — the `BracketReviser` composes these with
//! IBKR cancel-and-replace primitives. Keeps the math testable
//! without standing up the IBKR mock.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::utils::market_calendar;

/// Static spec attached to a setup. Lives on the
/// [`crate::strategies::exits::ExitPlan`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeStopSpec {
    pub max_trading_days: u32,
}

/// Resolve the calendar timestamp at which the time-stop fires given
/// the parent's fill time. Returns the 16:00 ET close on the trading
/// day `max_trading_days` business days after `entry_filled_at`'s ET
/// date. Weekends + holidays don't count toward the horizon — uses
/// the same `trading_days_after_close` helper the state machine pins
/// `in_play_until` against.
///
/// `max_trading_days = 0` returns the close on the same ET trading
/// day, which is the natural "intraday-only" mode a future detector
/// could pin without a special case.
pub fn deadline_for(entry_filled_at: DateTime<Utc>, spec: &TimeStopSpec) -> DateTime<Utc> {
    market_calendar::trading_days_after_close(entry_filled_at, spec.max_trading_days)
}

/// True iff `now >= deadline_for(entry_filled_at, spec)`. The
/// reviser polls this every minute during RTH; the first poll past
/// the deadline triggers a market-close on the remaining qty.
pub fn has_elapsed(
    now: DateTime<Utc>,
    entry_filled_at: DateTime<Utc>,
    spec: &TimeStopSpec,
) -> bool {
    now >= deadline_for(entry_filled_at, spec)
}

/// Trading days remaining from `now` to the deadline. `0` means we
/// are at or past the deadline. Used by the UI to render
/// "TimeStop: 3 BD remaining".
pub fn days_remaining(
    now: DateTime<Utc>,
    entry_filled_at: DateTime<Utc>,
    spec: &TimeStopSpec,
) -> u32 {
    use chrono_tz::America::New_York;
    let now_et = now.with_timezone(&New_York).date_naive();
    let deadline = deadline_for(entry_filled_at, spec)
        .with_timezone(&New_York)
        .date_naive();
    if now_et >= deadline {
        return 0;
    }
    market_calendar::trading_days_between(now_et, deadline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn et_close(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        // Anchor at 17:00 UTC (mid-RTH ET) so the calendar's
        // `today_et` resolves to the requested calendar date — passing
        // 00:00 UTC would push ET back a day. Then `trading_days_after_close(., 0)`
        // returns the requested date's RTH close.
        let anchor = Utc.with_ymd_and_hms(year, month, day, 17, 0, 0).unwrap();
        market_calendar::trading_days_after_close(anchor, 0)
    }

    #[test]
    fn deadline_advances_by_business_days_only() {
        // Friday 2026-05-01 fill at 13:30 UTC (≈ 09:30 ET) → 5 BD →
        // following Friday 2026-05-08 16:00 ET close.
        let fri = Utc.with_ymd_and_hms(2026, 5, 1, 13, 30, 0).unwrap();
        let deadline = deadline_for(
            fri,
            &TimeStopSpec {
                max_trading_days: 5,
            },
        );
        let expected = et_close(2026, 5, 8);
        assert_eq!(deadline, expected);
    }

    #[test]
    fn zero_horizon_means_same_session_close() {
        let mon = Utc.with_ymd_and_hms(2026, 5, 4, 14, 30, 0).unwrap();
        let deadline = deadline_for(
            mon,
            &TimeStopSpec {
                max_trading_days: 0,
            },
        );
        let expected = et_close(2026, 5, 4);
        assert_eq!(deadline, expected);
    }

    #[test]
    fn has_elapsed_true_at_or_after_deadline() {
        let mon = Utc.with_ymd_and_hms(2026, 5, 4, 14, 30, 0).unwrap();
        let spec = TimeStopSpec {
            max_trading_days: 3,
        };
        let deadline = deadline_for(mon, &spec);
        assert!(has_elapsed(deadline, mon, &spec));
        assert!(has_elapsed(
            deadline + chrono::Duration::seconds(1),
            mon,
            &spec
        ));
        assert!(!has_elapsed(
            deadline - chrono::Duration::seconds(1),
            mon,
            &spec
        ));
    }

    #[test]
    fn days_remaining_decreases_across_weekend() {
        // Fill on Wed 2026-04-29 at 14:30 UTC; horizon = 5 BD →
        // deadline Wed 2026-05-06 close. Saturday/Sunday don't count.
        let wed = Utc.with_ymd_and_hms(2026, 4, 29, 14, 30, 0).unwrap();
        let spec = TimeStopSpec {
            max_trading_days: 5,
        };
        let from_wed = days_remaining(wed, wed, &spec);
        // Same-day-as-fill: 5 BD between [wed → next-wed].
        assert_eq!(from_wed, 5);
        let fri = Utc.with_ymd_and_hms(2026, 5, 1, 14, 30, 0).unwrap();
        let from_fri = days_remaining(fri, wed, &spec);
        // Fri is 2 BD past Wed; 5 - 2 = 3 BD left.
        assert_eq!(from_fri, 3);
        let mon = Utc.with_ymd_and_hms(2026, 5, 4, 14, 30, 0).unwrap();
        let from_mon = days_remaining(mon, wed, &spec);
        // Mon = 3 BD past wed (fri+mon counted); 5 - 3 = 2.
        assert_eq!(from_mon, 2);
    }
}
