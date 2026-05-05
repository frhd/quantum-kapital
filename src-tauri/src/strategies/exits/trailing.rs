//! Phase 7 — chandelier trailing-stop computation + state.
//!
//! Pure functions only — the `BracketReviser` service composes these
//! with IBKR modify-order calls. Keeping the math here means tests
//! exercise the logic without standing up the IBKR mock.

use serde::{Deserialize, Serialize};

use crate::strategies::Direction;

/// Trail kind. Today only chandelier; reserved-form for a future
/// percent or fixed-cents trail without breaking the persisted JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailKind {
    Chandelier,
}

/// Static specification of the trail policy attached to a setup.
/// Stored on the [`crate::strategies::exits::ExitPlan`] and
/// re-read by the reviser to know how to step the stop child.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrailSpec {
    pub kind: TrailKind,
    /// Chandelier multiple — `max_high_since_entry - N×ATR` for longs.
    pub atr_multiple: f64,
    /// Activate trail only once the rung with this label has filled.
    /// `None` ↔ activate immediately on parent fill (not used by
    /// today's policies but reserved).
    pub activate_after_label: Option<String>,
    /// Move stop to break-even when the position has booked this many
    /// R of profit. `None` ↔ no BE move.
    pub move_to_break_even_at_r: Option<f64>,
}

/// Runtime state the reviser persists per active bracket. Written to
/// `bracket_groups.trail_state_json`. None ↔ no trail logic engaged
/// yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChandelierState {
    /// Highest high seen since parent fill (long) / lowest low (short).
    /// Updated each poll from the current quote / latest bar.
    pub extreme_price: f64,
    /// The last stop price the reviser computed and submitted to IBKR.
    /// Mirrors `bracket_groups.stop_price_cents` after a successful
    /// modify; a poll that tries to re-submit an unchanged price
    /// short-circuits before hitting IBKR.
    pub current_stop_price: f64,
    /// True once the trail has activated (the configured
    /// `activate_after_label` rung has filled or the BE move has
    /// fired). Pre-activation, the stop is the original.
    pub activated: bool,
    /// True if the BE move has fired. Once true, the stop never moves
    /// against the trader.
    pub be_moved: bool,
    /// RFC3339 UTC of the last successful modify_order call. Lets the
    /// status surface render "last trail step at HH:MM:SS".
    pub last_modify_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ChandelierState {
    pub fn new(initial_stop: f64) -> Self {
        Self {
            extreme_price: f64::NAN,
            current_stop_price: initial_stop,
            activated: false,
            be_moved: false,
            last_modify_at: None,
        }
    }
}

/// Compute the chandelier stop for a long: `max_high_since_entry -
/// atr_multiple × atr`. Short: `min_low_since_entry + atr_multiple ×
/// atr`. Never moves against the trader (returns
/// `max(prev_stop, computed)` for long, `min(prev_stop, computed)`
/// for short).
pub fn chandelier_stop(
    direction: Direction,
    extreme_price: f64,
    atr: f64,
    atr_multiple: f64,
    prev_stop: f64,
) -> f64 {
    if !extreme_price.is_finite() || !atr.is_finite() || !atr_multiple.is_finite() {
        return prev_stop;
    }
    let proposed = match direction {
        Direction::Long => extreme_price - atr_multiple * atr,
        Direction::Short => extreme_price + atr_multiple * atr,
    };
    match direction {
        Direction::Long => prev_stop.max(proposed),
        Direction::Short => prev_stop.min(proposed),
    }
}

/// True if the position has booked ≥ `r_threshold` of profit at
/// `current_price` given `entry_price` and `r_distance`. Used to
/// decide BE-move and trail activation.
pub fn has_reached_r(
    direction: Direction,
    entry_price: f64,
    r_distance: f64,
    current_price: f64,
    r_threshold: f64,
) -> bool {
    if !entry_price.is_finite() || !r_distance.is_finite() || !current_price.is_finite() {
        return false;
    }
    if r_distance <= 0.0 {
        return false;
    }
    let realized_r = match direction {
        Direction::Long => (current_price - entry_price) / r_distance,
        Direction::Short => (entry_price - current_price) / r_distance,
    };
    realized_r >= r_threshold
}

/// Update extreme_price from a new observation. Long → max; short →
/// min. NaN-aware so the first call seeds correctly.
pub fn updated_extreme(direction: Direction, prev: f64, observation: f64) -> f64 {
    if !observation.is_finite() {
        return prev;
    }
    if !prev.is_finite() {
        return observation;
    }
    match direction {
        Direction::Long => prev.max(observation),
        Direction::Short => prev.min(observation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chandelier_long_pulls_stop_up_with_higher_highs() {
        // Entry 100, original stop 98 (R=2). High climbs to 110, ATR
        // 1, multiplier 3 → trail = 110 - 3 = 107. Pulled up from 98.
        let s = chandelier_stop(Direction::Long, 110.0, 1.0, 3.0, 98.0);
        assert!((s - 107.0).abs() < 1e-9);
    }

    #[test]
    fn chandelier_long_never_moves_against_trader() {
        // Pre-existing trail at 105; new observation high is only 102
        // (drawdown), trail computed = 99. Must keep 105.
        let s = chandelier_stop(Direction::Long, 102.0, 1.0, 3.0, 105.0);
        assert!((s - 105.0).abs() < 1e-9);
    }

    #[test]
    fn chandelier_short_pulls_stop_down_with_lower_lows() {
        // Entry 100, stop 102 (R=2). Low drops to 90, ATR 1, mult 3
        // → trail = 90 + 3 = 93. Below stop 102.
        let s = chandelier_stop(Direction::Short, 90.0, 1.0, 3.0, 102.0);
        assert!((s - 93.0).abs() < 1e-9);
    }

    #[test]
    fn chandelier_short_never_moves_against_trader() {
        // Stop at 95 (already locked in). New low is 96, computed = 99.
        // Must keep 95.
        let s = chandelier_stop(Direction::Short, 96.0, 1.0, 3.0, 95.0);
        assert!((s - 95.0).abs() < 1e-9);
    }

    #[test]
    fn chandelier_with_nonfinite_inputs_returns_prev() {
        let s = chandelier_stop(Direction::Long, f64::NAN, 1.0, 3.0, 98.0);
        assert!((s - 98.0).abs() < 1e-9);
    }

    #[test]
    fn has_reached_r_long() {
        // Entry 100, R=2. Current 102 → realized R = 1.0.
        assert!(has_reached_r(Direction::Long, 100.0, 2.0, 102.0, 1.0));
        assert!(!has_reached_r(Direction::Long, 100.0, 2.0, 101.5, 1.0));
    }

    #[test]
    fn has_reached_r_short() {
        // Entry 100, R=2 (stop above). Current 98 → realized R = 1.0.
        assert!(has_reached_r(Direction::Short, 100.0, 2.0, 98.0, 1.0));
        assert!(!has_reached_r(Direction::Short, 100.0, 2.0, 99.5, 1.0));
    }

    #[test]
    fn updated_extreme_tracks_new_high_for_long() {
        let after_first = updated_extreme(Direction::Long, f64::NAN, 100.0);
        assert!((after_first - 100.0).abs() < 1e-9);
        let after_second = updated_extreme(Direction::Long, after_first, 102.0);
        assert!((after_second - 102.0).abs() < 1e-9);
        // Drawdown observation does not lower the high.
        let after_third = updated_extreme(Direction::Long, after_second, 99.0);
        assert!((after_third - 102.0).abs() < 1e-9);
    }

    #[test]
    fn updated_extreme_tracks_new_low_for_short() {
        let s1 = updated_extreme(Direction::Short, f64::NAN, 100.0);
        let s2 = updated_extreme(Direction::Short, s1, 98.0);
        let s3 = updated_extreme(Direction::Short, s2, 99.0);
        assert!((s3 - 98.0).abs() < 1e-9);
    }

    #[test]
    fn updated_extreme_ignores_nonfinite_observations() {
        let s = updated_extreme(Direction::Long, 100.0, f64::NAN);
        assert!((s - 100.0).abs() < 1e-9);
    }
}
