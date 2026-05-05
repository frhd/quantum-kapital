//! Phase 7 — legacy static 50/30/20 ladder at 1R / 2R / 3R.
//!
//! Retained for the 4-week shadow run committed in master ("compare
//! `atr_scaled` head-to-head with `static_2r_3r`"). On `cargo test`
//! against the policy it reproduces the same prices the Phase 3
//! `build_static_target_ladder` helper produced — the bracket placer
//! switches from calling that helper to calling this trait, so the
//! pre-P7 wire-shape is preserved exactly under `v1_static`.

use super::{
    signed_multiplier, validate_geometry, ExitPlan, ExitPolicy, ExitPolicyContext,
    ExitPolicyError, ExitTargetSpec, Result, V1_STATIC,
};

/// 50/30/20 of parent qty at 1R, 2R, 3R. No trail, no time stop.
#[derive(Debug, Clone, Copy, Default)]
pub struct StaticTwoRThreeR;

const STATIC_PCTS: [u8; 3] = [50, 30, 20];
const STATIC_R_MULTIPLES: [f64; 3] = [1.0, 2.0, 3.0];

impl ExitPolicy for StaticTwoRThreeR {
    fn version(&self) -> &'static str {
        V1_STATIC
    }

    fn build_plan(&self, ctx: &ExitPolicyContext<'_>) -> Result<ExitPlan> {
        let r = validate_geometry(ctx.trigger_price, ctx.stop_price)?;
        let signed = signed_multiplier(ctx.direction);

        let mut targets = Vec::with_capacity(STATIC_PCTS.len());
        for (idx, (&pct, &mult)) in STATIC_PCTS.iter().zip(STATIC_R_MULTIPLES.iter()).enumerate() {
            targets.push(ExitTargetSpec {
                label: format!("{:.0}R", mult),
                price: ctx.trigger_price + signed * mult * r,
                qty_pct: pct,
                r_multiple: Some(mult),
                atr_multiple: None,
            });
            // Belt + suspenders: panic in dev if pcts/mults drift apart.
            debug_assert!(idx < STATIC_PCTS.len());
        }

        Ok(ExitPlan {
            policy_version: V1_STATIC.to_string(),
            targets,
            trail: None,
            time_stop: None,
            atr_at_signal: ctx.atr,
        })
    }
}

/// Convenience for the registry default — exposes the geometry guard
/// without going through the trait. Kept here (not on the trait) so
/// the cancel-revert path can reach it without dynamic dispatch.
#[allow(dead_code)]
pub(crate) fn build_static_plan(
    direction: crate::strategies::Direction,
    trigger_price: f64,
    stop_price: f64,
) -> std::result::Result<ExitPlan, ExitPolicyError> {
    StaticTwoRThreeR.build_plan(&ExitPolicyContext {
        direction,
        trigger_price,
        stop_price,
        atr: None,
        strategy: "",
    })
}
