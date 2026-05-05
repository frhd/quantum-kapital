//! Phase 1 — pure sizing math.
//!
//! `compute_sizing` is the single source of truth for "given a
//! candidate, an equity snapshot, and a config, how many shares?".
//! Side-effect-free; the only dependency is `f64` arithmetic so
//! every reference case below is a fixture-as-code.
//!
//! Every rounding step floors. Floor-then-cap matches retail brokers'
//! whole-share execution and avoids accidentally over-risking when a
//! cap binds.

use crate::strategies::{Direction, SetupCandidate};

use super::types::{
    ConvictionGrade, EquitySnapshot, RiskConfig, Sizing, SizingSkippedReason, SIZING_VERSION,
};

/// Multiplier applied on top of the per-grade `risk_pct`. P1 caps
/// at 1.0× (master decision). Future P4 calibration unlocks > 1.0
/// for A-conviction once realized hit-rate hits the target.
fn conviction_multiplier(_grade: ConvictionGrade, cfg: &RiskConfig) -> f64 {
    1.0_f64.min(cfg.conviction_multiplier_cap.max(0.0))
}

fn round_to_lot(qty: u32, lot: u32) -> u32 {
    let lot = lot.max(1);
    (qty / lot) * lot
}

fn cents(usd: f64) -> i64 {
    (usd * 100.0).round() as i64
}

/// Compute `Sizing` for `candidate` against `snapshot` under `cfg`.
/// Pure: no IO, no clock reads, no logging. The risk engine wraps
/// this with snapshot lookup and persistence.
pub fn compute_sizing(
    candidate: &SetupCandidate,
    snapshot: &EquitySnapshot,
    cfg: &RiskConfig,
) -> Sizing {
    let trigger = candidate.trigger_price;
    let stop = candidate.stop_price;
    let grade = ConvictionGrade::from_signal(candidate.conviction_signal);
    let equity_cents = snapshot.nlv_cents;

    if !trigger.is_finite() || !stop.is_finite() || trigger <= 0.0 || stop <= 0.0 {
        return Sizing::skipped(SizingSkippedReason::InvalidPrice, equity_cents, grade);
    }

    // Sanity: long expects stop < trigger; short expects stop >
    // trigger. A violation is a detector bug; refuse to size rather
    // than silently inverting risk.
    let directionally_valid = match candidate.direction {
        Direction::Long => stop < trigger,
        Direction::Short => stop > trigger,
    };
    if !directionally_valid {
        return Sizing::skipped(SizingSkippedReason::InvalidPrice, equity_cents, grade);
    }

    let r_per_share = (trigger - stop).abs();
    if r_per_share <= 0.0 || !r_per_share.is_finite() {
        return Sizing::skipped(SizingSkippedReason::ZeroR, equity_cents, grade);
    }

    let multiplier = conviction_multiplier(grade, cfg);
    let multiplier_bps = (multiplier * 10_000.0).round() as u32;
    let risk_pct = cfg.risk_pct_for(grade).max(0.0);
    let equity = snapshot.nlv();
    if equity <= 0.0 {
        // No equity → can't size. Treat as below-min-risk so the UI
        // still surfaces a clear reason.
        return Sizing::skipped(SizingSkippedReason::BelowMinRisk, equity_cents, grade);
    }

    let target_dollar_risk = equity * risk_pct * multiplier;
    if target_dollar_risk < cfg.min_dollar_risk {
        return Sizing::skipped(SizingSkippedReason::BelowMinRisk, equity_cents, grade);
    }

    let raw_qty = (target_dollar_risk / r_per_share).floor();
    let mut qty = if raw_qty.is_finite() && raw_qty > 0.0 {
        raw_qty as u32
    } else {
        0
    };
    qty = round_to_lot(qty, cfg.round_lot);

    // Notional cap: low-vol stocks blow past dollar-risk math.
    let max_notional = equity * cfg.max_position_pct.max(0.0);
    let mut cap_applied = false;
    let notional_at_qty = qty as f64 * trigger;
    if max_notional > 0.0 && notional_at_qty > max_notional {
        let capped = (max_notional / trigger).floor();
        let mut capped_qty = if capped.is_finite() && capped > 0.0 {
            capped as u32
        } else {
            0
        };
        capped_qty = round_to_lot(capped_qty, cfg.round_lot);
        qty = capped_qty;
        cap_applied = true;
    }

    if qty == 0 {
        return Sizing::skipped(SizingSkippedReason::BelowMinRisk, equity_cents, grade);
    }

    let dollar_risk = qty as f64 * r_per_share;
    Sizing {
        qty,
        dollar_risk_cents: cents(dollar_risk),
        r_per_share_cents: cents(r_per_share),
        equity_at_decision_cents: equity_cents,
        conviction_grade: grade,
        conviction_multiplier_bps: multiplier_bps,
        cap_applied,
        skipped_reason: None,
        version: SIZING_VERSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::{BarSize, StrategyTag};
    use crate::services::risk_engine::types::EquitySource;
    use chrono::Utc;
    use serde_json::json;

    fn snapshot(nlv: f64) -> EquitySnapshot {
        EquitySnapshot {
            account: "DU1".to_string(),
            as_of_date: "2026-05-04".to_string(),
            nlv_cents: cents(nlv),
            source: EquitySource::IbkrAccountSummary,
            fetched_at: Utc::now(),
        }
    }

    fn candidate(signal: f64, direction: Direction, trigger: f64, stop: f64) -> SetupCandidate {
        SetupCandidate {
            strategy: "breakout",
            tag: StrategyTag::Breakout,
            direction,
            conviction_signal: signal,
            trigger_price: trigger,
            stop_price: stop,
            targets: Vec::new(),
            raw_signals: json!({}),
            timeframe: BarSize::Day1,
            detected_at: Utc::now(),
        }
    }

    // --- A/B/C reference cases ---

    #[test]
    fn a_conviction_long_50bp_risk_at_100k_equity() {
        // Equity 100_000, risk 0.5% = $500. R = $5/share. Qty = 100.
        let snap = snapshot(100_000.0);
        let cand = candidate(0.9, Direction::Long, 105.0, 100.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert_eq!(s.skipped_reason, None);
        assert_eq!(s.conviction_grade, ConvictionGrade::A);
        assert_eq!(s.qty, 100);
        assert_eq!(s.dollar_risk_cents, 50_000);
        assert_eq!(s.r_per_share_cents, 500);
        assert_eq!(s.conviction_multiplier_bps, 10_000);
        assert!(!s.cap_applied);
    }

    #[test]
    fn b_conviction_long_uses_33bp_risk() {
        // Equity 100_000, B = 0.33% = $330. R = $5. Qty floor(330/5) = 66.
        let snap = snapshot(100_000.0);
        let cand = candidate(0.6, Direction::Long, 105.0, 100.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert_eq!(s.conviction_grade, ConvictionGrade::B);
        assert_eq!(s.qty, 66);
        assert_eq!(s.dollar_risk_cents, 33_000);
    }

    #[test]
    fn c_conviction_long_uses_16bp_risk() {
        // Equity 100_000, C = 0.16% = $160. R = $5. Qty floor(160/5) = 32.
        let snap = snapshot(100_000.0);
        let cand = candidate(0.2, Direction::Long, 105.0, 100.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert_eq!(s.conviction_grade, ConvictionGrade::C);
        assert_eq!(s.qty, 32);
        assert_eq!(s.dollar_risk_cents, 16_000);
    }

    // --- short side ---

    #[test]
    fn short_uses_stop_above_trigger() {
        // Equity 100_000, A = $500. Short trigger 100, stop 105, R = $5. Qty 100.
        let snap = snapshot(100_000.0);
        let cand = candidate(0.9, Direction::Short, 100.0, 105.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert_eq!(s.qty, 100);
        assert_eq!(s.dollar_risk_cents, 50_000);
        assert!(!s.cap_applied);
    }

    // --- stop-distance variations ---

    #[test]
    fn tighter_stop_yields_more_shares() {
        let snap = snapshot(100_000.0);
        let wide = compute_sizing(
            &candidate(0.9, Direction::Long, 105.0, 100.0),
            &snap,
            &RiskConfig::default(),
        );
        let tight = compute_sizing(
            &candidate(0.9, Direction::Long, 105.0, 104.0),
            &snap,
            &RiskConfig::default(),
        );
        assert!(tight.qty > wide.qty);
        // dollar-risk stays at-or-under target for both.
        assert!(tight.dollar_risk_cents <= 50_000);
        assert!(wide.dollar_risk_cents <= 50_000);
    }

    // --- equity cap ---

    #[test]
    fn max_position_pct_caps_low_vol_size() {
        // Tight stop on a low-vol $50 stock: A = $500 / $0.10 = 5000 sh.
        // Notional = 5000 * 50 = $250k > 25% of $100k = $25k cap.
        // Capped qty = 25_000 / 50 = 500.
        let snap = snapshot(100_000.0);
        let cand = candidate(0.9, Direction::Long, 50.0, 49.90);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert!(s.cap_applied);
        assert_eq!(s.qty, 500);
    }

    // --- minimum dollar-risk floor ---

    #[test]
    fn below_min_risk_skips_with_reason() {
        // Equity 1_000, C = 0.16% = $1.60 — below $10 floor.
        let snap = snapshot(1_000.0);
        let cand = candidate(0.2, Direction::Long, 105.0, 100.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert_eq!(s.skipped_reason, Some(SizingSkippedReason::BelowMinRisk));
        assert_eq!(s.qty, 0);
        assert_eq!(s.equity_at_decision_cents, 100_000);
    }

    // --- defensive guards ---

    #[test]
    fn zero_r_skips_when_trigger_equals_stop() {
        // Defensive: trigger == stop is invalid (directional check
        // also fails before zero-R check).
        let snap = snapshot(100_000.0);
        let cand = candidate(0.9, Direction::Long, 100.0, 100.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert!(matches!(
            s.skipped_reason,
            Some(SizingSkippedReason::InvalidPrice | SizingSkippedReason::ZeroR)
        ));
        assert_eq!(s.qty, 0);
    }

    #[test]
    fn long_with_stop_above_trigger_is_invalid() {
        let snap = snapshot(100_000.0);
        let cand = candidate(0.9, Direction::Long, 100.0, 105.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert_eq!(s.skipped_reason, Some(SizingSkippedReason::InvalidPrice));
    }

    #[test]
    fn nan_trigger_is_rejected() {
        let snap = snapshot(100_000.0);
        let cand = candidate(0.9, Direction::Long, f64::NAN, 100.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert_eq!(s.skipped_reason, Some(SizingSkippedReason::InvalidPrice));
    }

    #[test]
    fn zero_equity_skips_below_min_risk() {
        let snap = snapshot(0.0);
        let cand = candidate(0.9, Direction::Long, 105.0, 100.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert_eq!(s.skipped_reason, Some(SizingSkippedReason::BelowMinRisk));
    }

    // --- conviction multiplier cap ---

    #[test]
    fn conviction_multiplier_cap_clamps_to_one_x() {
        // Even with a config setting cap = 2.0, P1 logic clamps to 1.0
        // because the master decision pins it pre-P4. (`min(1.0, cap)`.)
        let snap = snapshot(100_000.0);
        let cfg = RiskConfig {
            conviction_multiplier_cap: 2.0,
            ..RiskConfig::default()
        };
        let s = compute_sizing(&candidate(0.9, Direction::Long, 105.0, 100.0), &snap, &cfg);
        assert_eq!(s.conviction_multiplier_bps, 10_000);
        assert_eq!(s.qty, 100);
    }

    // --- round lots ---

    #[test]
    fn round_lot_floors_to_multiple() {
        let snap = snapshot(100_000.0);
        // A risk = 100 sh on the reference case; 100 % 10 == 0.
        // Tighten risk_pct_a to force a non-multiple before rounding:
        // 0.41% -> $410, R $5 -> floor(82) -> rounded down to 80.
        let cfg = RiskConfig {
            round_lot: 10,
            risk_pct_a: 0.0041,
            ..RiskConfig::default()
        };
        let s = compute_sizing(&candidate(0.9, Direction::Long, 105.0, 100.0), &snap, &cfg);
        assert_eq!(s.qty, 80);
        assert_eq!(s.dollar_risk_cents, 40_000);
    }

    // --- skipped variants are not silently sized ---

    #[test]
    fn skipped_sizing_records_equity_and_grade() {
        let snap = snapshot(100.0);
        let cand = candidate(0.9, Direction::Long, 105.0, 100.0);
        let s = compute_sizing(&cand, &snap, &RiskConfig::default());
        assert!(s.is_skipped());
        assert_eq!(s.equity_at_decision_cents, 10_000);
        assert_eq!(s.conviction_grade, ConvictionGrade::A);
    }
}
