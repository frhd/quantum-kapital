//! Phase 7 — integration tests across exit-policy impls.
//!
//! Per-impl unit tests live alongside each impl. This file exercises
//! cross-impl invariants:
//!  - both policies sum qty_pct to 100
//!  - both policies preserve direction-aware target geometry
//!  - the registry default dispatches by strategy name
//!  - `v1_static` and `v2_atr_scaled` produce different price ladders
//!    (so the shadow comparison would be observable)

use super::*;
use crate::strategies::Direction;

fn long_ctx_with_atr(strategy: &'static str) -> ExitPolicyContext<'static> {
    ExitPolicyContext {
        direction: Direction::Long,
        trigger_price: 100.0,
        stop_price: 98.0, // R = 2.0
        atr: Some(1.5),
        strategy,
    }
}

fn short_ctx_with_atr(strategy: &'static str) -> ExitPolicyContext<'static> {
    ExitPolicyContext {
        direction: Direction::Short,
        trigger_price: 100.0,
        stop_price: 102.0, // R = 2.0
        atr: Some(1.5),
        strategy,
    }
}

#[test]
fn static_long_produces_1r_2r_3r_ladder() {
    let plan = StaticTwoRThreeR
        .build_plan(&long_ctx_with_atr("breakout"))
        .unwrap();
    assert_eq!(plan.policy_version, V1_STATIC);
    assert_eq!(plan.targets.len(), 3);
    assert_eq!(
        plan.targets.iter().map(|t| t.qty_pct as u32).sum::<u32>(),
        100
    );
    // R = 2.0; 1R/2R/3R targets at 102/104/106.
    assert!((plan.targets[0].price - 102.0).abs() < 1e-9);
    assert!((plan.targets[1].price - 104.0).abs() < 1e-9);
    assert!((plan.targets[2].price - 106.0).abs() < 1e-9);
    assert!(plan.trail.is_none());
    assert!(plan.time_stop.is_none());
}

#[test]
fn static_short_inverts_target_direction() {
    let plan = StaticTwoRThreeR
        .build_plan(&short_ctx_with_atr("parabolic_short"))
        .unwrap();
    // R = 2.0 short — targets below trigger.
    assert!((plan.targets[0].price - 98.0).abs() < 1e-9);
    assert!((plan.targets[1].price - 96.0).abs() < 1e-9);
    assert!((plan.targets[2].price - 94.0).abs() < 1e-9);
}

#[test]
fn static_rejects_zero_r() {
    let p = StaticTwoRThreeR.build_plan(&ExitPolicyContext {
        direction: Direction::Long,
        trigger_price: 100.0,
        stop_price: 100.0,
        atr: None,
        strategy: "breakout",
    });
    assert!(matches!(p, Err(ExitPolicyError::InvalidGeometry)));
}

#[test]
fn atr_scaled_long_produces_atr_ladder() {
    let plan = AtrScaled::new(10)
        .build_plan(&long_ctx_with_atr("breakout"))
        .unwrap();
    assert_eq!(plan.policy_version, V2_ATR_SCALED);
    assert_eq!(plan.targets.len(), 3);
    // 1×ATR / 2×ATR / 4×ATR with ATR=1.5 → +1.5, +3.0, +6.0.
    assert!((plan.targets[0].price - 101.5).abs() < 1e-9);
    assert!((plan.targets[1].price - 103.0).abs() < 1e-9);
    assert!((plan.targets[2].price - 106.0).abs() < 1e-9);
    assert_eq!(
        plan.targets.iter().map(|t| t.qty_pct as u32).sum::<u32>(),
        100
    );
    assert!(plan.trail.is_some());
    let trail = plan.trail.unwrap();
    assert!((trail.atr_multiple - 3.0).abs() < 1e-9);
    assert_eq!(trail.move_to_break_even_at_r, Some(1.0));
    assert_eq!(plan.time_stop.unwrap().max_trading_days, 10);
    assert_eq!(plan.atr_at_signal, Some(1.5));
}

#[test]
fn atr_scaled_short_inverts() {
    let plan = AtrScaled::new(3)
        .build_plan(&short_ctx_with_atr("parabolic_short"))
        .unwrap();
    // 1×/2×/4× ATR below 100.
    assert!((plan.targets[0].price - 98.5).abs() < 1e-9);
    assert!((plan.targets[1].price - 97.0).abs() < 1e-9);
    assert!((plan.targets[2].price - 94.0).abs() < 1e-9);
    assert_eq!(plan.time_stop.unwrap().max_trading_days, 3);
}

#[test]
fn atr_scaled_refuses_when_atr_missing() {
    let mut ctx = long_ctx_with_atr("breakout");
    ctx.atr = None;
    let p = AtrScaled::new(10).build_plan(&ctx);
    assert!(matches!(p, Err(ExitPolicyError::AtrUnavailable(_))));
}

#[test]
fn atr_scaled_refuses_when_atr_nonpositive() {
    let mut ctx = long_ctx_with_atr("breakout");
    ctx.atr = Some(0.0);
    assert!(matches!(
        AtrScaled::new(10).build_plan(&ctx),
        Err(ExitPolicyError::AtrUnavailable(_))
    ));
    let mut ctx2 = long_ctx_with_atr("breakout");
    ctx2.atr = Some(f64::NAN);
    assert!(matches!(
        AtrScaled::new(10).build_plan(&ctx2),
        Err(ExitPolicyError::AtrUnavailable(_))
    ));
}

#[test]
fn registry_default_dispatches_by_strategy() {
    let reg = ExitPolicyRegistry::default_for_phase_7();
    let breakout = reg.for_strategy("breakout");
    assert_eq!(breakout.version(), V2_ATR_SCALED);
    let unknown = reg.for_strategy("not_a_real_strategy");
    assert_eq!(unknown.version(), V1_STATIC);
}

#[test]
fn registry_all_static_returns_v1_for_all() {
    let reg = ExitPolicyRegistry::all_static();
    for strat in ["breakout", "episodic_pivot", "parabolic_short", "x"] {
        assert_eq!(reg.for_strategy(strat).version(), V1_STATIC);
    }
}

#[test]
fn time_stop_horizon_per_detector_matches_master() {
    let reg = ExitPolicyRegistry::default_for_phase_7();
    let cases = [
        ("breakout", 10),
        ("episodic_pivot", 5),
        ("parabolic_short", 3),
    ];
    for (strat, days) in cases {
        let plan = reg
            .for_strategy(strat)
            .build_plan(&long_ctx_with_atr(strat))
            .unwrap();
        assert_eq!(
            plan.time_stop
                .expect("atr_scaled emits time_stop")
                .max_trading_days,
            days,
            "{strat} expected {days} BD time-stop horizon"
        );
    }
}

#[test]
fn shadow_policies_produce_different_prices_for_observable_a_b() {
    // The shadow comparison hinges on `atr_scaled` and `static` not
    // collapsing to the same ladder for the typical R≠ATR case.
    // Pick R = 2.0, ATR = 1.5 → static targets {102, 104, 106}, ATR
    // targets {101.5, 103.0, 106.0}. Distinct on rungs 0/1.
    let ctx = long_ctx_with_atr("breakout");
    let static_plan = StaticTwoRThreeR.build_plan(&ctx).unwrap();
    let atr_plan = AtrScaled::new(10).build_plan(&ctx).unwrap();
    assert_ne!(static_plan.targets[0].price, atr_plan.targets[0].price);
    assert_ne!(static_plan.targets[1].price, atr_plan.targets[1].price);
    // Conicidentally identical for rung 2 with these specific
    // numbers (3R = 4×ATR = 6.0). That's fine — the shadow looks at
    // the ladder, not just one rung.
}
