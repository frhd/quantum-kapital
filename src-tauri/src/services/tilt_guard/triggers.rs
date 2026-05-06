//! Phase 11 — pure trigger evaluation.
//!
//! Walks today's R-stream chronologically and returns the first
//! threshold breach. Two rules, both committed by master:
//!
//!   - Cumulative R reaches `cum_r_threshold` (default `-3.0`). Day-N+1
//!     after a manual override raises this to `-2.0` via the
//!     `effective_cum_r_threshold` helper.
//!   - `consecutive_loss_threshold` consecutive closed losses (default
//!     `2`). A winner resets the streak — see the master gotcha
//!     "winning recovery day".
//!
//! No state — given the same input slice and config, returns the same
//! `Option<TriggerEval>`. Real I/O lives in `mod.rs`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One entry in the R-stream. Realized R = leg `net_pnl / dollar_risk`,
/// computed by the caller from `executions` joined to `setups`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosedTrade {
    pub closed_at: DateTime<Utc>,
    pub realized_r: f64,
}

/// Which rule fired first when walking the R-stream. Stored on
/// `tilt_episodes.trigger_kind` as the `as_str()` form so a future
/// trigger doesn't need a migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    /// Cumulative R for the day reached the configured floor.
    CumRNegative,
    /// `consecutive_loss_threshold` losing closed trades back-to-back.
    TwoConsecutiveLosses,
}

impl TriggerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerKind::CumRNegative => "cum_r_negative",
            TriggerKind::TwoConsecutiveLosses => "two_consecutive_losses",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cum_r_negative" => Some(TriggerKind::CumRNegative),
            "two_consecutive_losses" => Some(TriggerKind::TwoConsecutiveLosses),
            _ => None,
        }
    }
}

/// Snapshot of the trigger that fired. Cumulative-R at the firing
/// trade and the consecutive-loss count are captured so the audit row
/// + UI banner can show "you tilted at -3.2R after 2 consecutive
///   losses" without re-walking the stream.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerEval {
    pub kind: TriggerKind,
    pub cumulative_r: f64,
    pub consecutive_losses: u32,
    pub triggered_at: DateTime<Utc>,
}

/// Walk the R-stream in time order; return the first rule that fires.
///
/// Pre-conditions: `closed_trades` must be sorted ascending by
/// `closed_at`. Empty input returns `None`. NaN realized-R values are
/// treated as zero (no contribution) so a malformed leg can't tilt the
/// account by accident.
pub fn evaluate_triggers(
    closed_trades: &[ClosedTrade],
    cum_r_threshold: f64,
    consecutive_loss_threshold: u32,
) -> Option<TriggerEval> {
    let mut cum: f64 = 0.0;
    let mut streak: u32 = 0;
    for t in closed_trades {
        let r = if t.realized_r.is_finite() {
            t.realized_r
        } else {
            0.0
        };
        cum += r;
        if r < 0.0 {
            streak += 1;
        } else {
            streak = 0;
        }
        if cum <= cum_r_threshold {
            return Some(TriggerEval {
                kind: TriggerKind::CumRNegative,
                cumulative_r: cum,
                consecutive_losses: streak,
                triggered_at: t.closed_at,
            });
        }
        if streak >= consecutive_loss_threshold {
            return Some(TriggerEval {
                kind: TriggerKind::TwoConsecutiveLosses,
                cumulative_r: cum,
                consecutive_losses: streak,
                triggered_at: t.closed_at,
            });
        }
    }
    None
}

/// Day-N+1 stricter threshold. Master decision: an override on day N
/// raises day N+1's threshold by 1R for that day only. Caller passes
/// `prev_day_overridden = true` when the most-recent prior trading
/// day had a `release_kind = 'manual_override'` episode.
pub fn effective_cum_r_threshold(base_threshold: f64, prev_day_overridden: bool) -> f64 {
    if prev_day_overridden {
        base_threshold + 1.0
    } else {
        base_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: i64, r: f64) -> ClosedTrade {
        ClosedTrade {
            closed_at: chrono::DateTime::from_timestamp(secs, 0).unwrap(),
            realized_r: r,
        }
    }

    #[test]
    fn empty_stream_does_not_trigger() {
        assert!(evaluate_triggers(&[], -3.0, 2).is_none());
    }

    #[test]
    fn cum_r_minus_three_triggers_on_third_loss() {
        let trades = vec![t(1, -1.0), t(2, -1.0), t(3, -1.0)];
        let ev = evaluate_triggers(&trades, -3.0, 99).unwrap();
        assert_eq!(ev.kind, TriggerKind::CumRNegative);
        assert!((ev.cumulative_r + 3.0).abs() < 1e-9);
    }

    #[test]
    fn two_consecutive_losses_triggers() {
        let trades = vec![t(1, -0.5), t(2, -0.5)];
        let ev = evaluate_triggers(&trades, -10.0, 2).unwrap();
        assert_eq!(ev.kind, TriggerKind::TwoConsecutiveLosses);
        assert_eq!(ev.consecutive_losses, 2);
    }

    #[test]
    fn winner_in_middle_resets_consecutive_streak() {
        let trades = vec![t(1, -1.0), t(2, 2.0), t(3, -1.0)];
        // 2 closed losses but not consecutive — no two-in-a-row trigger;
        // cumulative is 0R so cum-R doesn't fire either.
        assert!(evaluate_triggers(&trades, -3.0, 2).is_none());
    }

    #[test]
    fn nan_r_is_treated_as_zero() {
        let trades = vec![t(1, f64::NAN), t(2, -2.0), t(3, -2.0)];
        // NaN doesn't count as a loss: streak only reaches 2 after the
        // last two real losses; cumulative is -4 ≤ -3 so cum-R fires
        // first at the third trade.
        let ev = evaluate_triggers(&trades, -3.0, 2).unwrap();
        assert_eq!(ev.kind, TriggerKind::CumRNegative);
    }

    #[test]
    fn cum_r_fires_before_consecutive_when_both_cross_same_trade() {
        // Two -2R closes: cum = -4 (≤ -3) AND consecutive = 2. Master
        // decision is implementation-defined here; we pin cum-R first
        // so a single big loss can't be misclassified as a streak.
        let trades = vec![t(1, -2.0), t(2, -2.0)];
        let ev = evaluate_triggers(&trades, -3.0, 2).unwrap();
        assert_eq!(ev.kind, TriggerKind::CumRNegative);
    }

    #[test]
    fn stricter_threshold_after_override() {
        assert_eq!(effective_cum_r_threshold(-3.0, false), -3.0);
        assert_eq!(effective_cum_r_threshold(-3.0, true), -2.0);
    }

    #[test]
    fn stricter_threshold_fires_at_minus_two_after_override() {
        let trades = vec![t(1, -1.0), t(2, -1.0)];
        // Base threshold -3 wouldn't fire on -2 cum, but the
        // post-override day uses -2.
        assert!(evaluate_triggers(&trades, -3.0, 99).is_none());
        assert!(evaluate_triggers(&trades, effective_cum_r_threshold(-3.0, true), 99).is_some());
    }
}
