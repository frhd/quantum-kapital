//! Phase 7 — vol-adjusted exits.
//!
//! `ExitPolicy` is the per-detector decision rail that replaces the
//! Phase 3 hardcoded 50/30/20 ladder at 1R / 2R / 3R. Two impls ship:
//!
//! - [`StaticTwoRThreeR`] (`v1_static`) — the legacy ladder, retained
//!   so a 4-week shadow run can compare side-by-side and master's
//!   "neither passes → keep static" branch is reachable without
//!   redeploying.
//! - [`AtrScaled`] (`v2_atr_scaled`) — 1× / 2× / 4× ATR(20) at signal
//!   time, with chandelier ATR-trail on the runner and BE-move at 1R.
//!   Default policy in P7.
//!
//! The trait returns an [`ExitPlan`] — a frozen-at-signal-time bundle
//! of target rungs, optional trail spec, and optional time-stop spec.
//! The plan is persisted on the setup row (`exit_plan_json`) before
//! the trader sees the take-setup modal, so the bracket placer
//! reproduces exactly what the runner promised; the
//! `BracketReviser` reads the trail spec to know how to step the
//! stop child during RTH.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

use crate::strategies::Direction;

pub mod atr_scaled;
pub mod static_2r_3r;
pub mod time_stop;
pub mod trailing;

#[cfg(test)]
mod tests;

pub use atr_scaled::AtrScaled;
pub use static_2r_3r::StaticTwoRThreeR;
pub use time_stop::TimeStopSpec;
pub use trailing::{
    chandelier_stop, has_reached_r, updated_extreme, ChandelierState, TrailKind, TrailSpec,
};

/// Stable string ids written to `setups.exit_policy_version`. Master
/// invariant 5: never translate between versions at read time — newer
/// rows declare their version, older rows fall through to
/// `V1_STATIC`.
pub const V1_STATIC: &str = "v1_static";
pub const V2_ATR_SCALED: &str = "v2_atr_scaled";

/// Inputs the runner hands to the policy at signal-detection time.
/// Borrowed because the candidate is short-lived; the policy returns
/// owned [`ExitPlan`] data.
#[derive(Debug, Clone)]
pub struct ExitPolicyContext<'a> {
    pub direction: Direction,
    pub trigger_price: f64,
    pub stop_price: f64,
    /// Daily ATR(20) at signal time. `None` ↔ insufficient bars; the
    /// ATR-scaled policy refuses with [`ExitPolicyError::AtrUnavailable`]
    /// in that case so the runner falls back to static rather than
    /// silently fabricate target prices.
    pub atr: Option<f64>,
    pub strategy: &'a str,
}

/// One rung of the exit ladder. `qty_pct` is the share of parent qty;
/// the bracket placer materializes whole-share `qty` from the modal.
/// `r_multiple` and `atr_multiple` are advisory metadata so the UI
/// can label the rung ("2R", "1×ATR runner") without re-deriving from
/// price + trigger + stop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitTargetSpec {
    pub label: String,
    pub price: f64,
    pub qty_pct: u8,
    pub r_multiple: Option<f64>,
    pub atr_multiple: Option<f64>,
}

/// Frozen-at-signal-time exit plan. Written to
/// `setups.exit_plan_json` and re-read at confirm time. The ladder,
/// trail and time-stop together fully describe the policy; the
/// bracket placer and reviser are pure functions of this plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitPlan {
    pub policy_version: String,
    pub targets: Vec<ExitTargetSpec>,
    pub trail: Option<TrailSpec>,
    pub time_stop: Option<TimeStopSpec>,
    /// Snapshot of the ATR used to compute prices. Lets the reviser
    /// recompute trail steps in the same units the policy chose at
    /// signal time, without re-fetching bars.
    pub atr_at_signal: Option<f64>,
}

#[derive(Debug, Error)]
pub enum ExitPolicyError {
    #[error("trigger and stop must be finite, non-equal")]
    InvalidGeometry,
    #[error("ATR(20) unavailable — policy {0} requires it")]
    AtrUnavailable(&'static str),
}

pub type Result<T> = std::result::Result<T, ExitPolicyError>;

/// The trait. Stateless — every call rebuilds a plan from the
/// context. Impls implement [`Self::version`] + [`Self::build_plan`].
pub trait ExitPolicy: Send + Sync + std::fmt::Debug {
    fn version(&self) -> &'static str;
    fn build_plan(&self, ctx: &ExitPolicyContext<'_>) -> Result<ExitPlan>;
}

/// Per-detector exit-policy lookup. Constructed once from settings
/// (or from `default_for_phase_7` until the settings surface is
/// wired) and kept on the runner. Resolves a strategy name to its
/// configured policy; falls back to [`StaticTwoRThreeR`] when the
/// strategy isn't registered.
#[derive(Clone)]
pub struct ExitPolicyRegistry {
    map: Arc<std::collections::HashMap<String, Arc<dyn ExitPolicy>>>,
    default_policy: Arc<dyn ExitPolicy>,
}

impl ExitPolicyRegistry {
    pub fn new(
        map: std::collections::HashMap<String, Arc<dyn ExitPolicy>>,
        default_policy: Arc<dyn ExitPolicy>,
    ) -> Self {
        Self {
            map: Arc::new(map),
            default_policy,
        }
    }

    /// Returns the policy registered for `strategy`, or the registry
    /// default. Never `None` — callers always get an executable plan
    /// builder.
    pub fn for_strategy(&self, strategy: &str) -> Arc<dyn ExitPolicy> {
        self.map
            .get(strategy)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&self.default_policy))
    }

    /// Phase 7 default registry: ATR-scaled for the three live
    /// detectors, with per-detector time-stop horizons committed in
    /// master.
    ///
    /// - breakout: 10 trading days
    /// - episodic_pivot: 5 trading days
    /// - parabolic_short: 3 trading days
    pub fn default_for_phase_7() -> Self {
        use std::collections::HashMap;
        let mut map: HashMap<String, Arc<dyn ExitPolicy>> = HashMap::new();
        map.insert(
            "breakout".to_string(),
            Arc::new(AtrScaled::new(10)) as Arc<dyn ExitPolicy>,
        );
        map.insert(
            "episodic_pivot".to_string(),
            Arc::new(AtrScaled::new(5)) as Arc<dyn ExitPolicy>,
        );
        map.insert(
            "parabolic_short".to_string(),
            Arc::new(AtrScaled::new(3)) as Arc<dyn ExitPolicy>,
        );
        let default = Arc::new(StaticTwoRThreeR) as Arc<dyn ExitPolicy>;
        Self::new(map, default)
    }

    /// Pre-P7 fallback: every strategy gets the legacy static ladder.
    /// Useful for tests + the shadow-mode comparator.
    pub fn all_static() -> Self {
        let default = Arc::new(StaticTwoRThreeR) as Arc<dyn ExitPolicy>;
        Self::new(std::collections::HashMap::new(), default)
    }
}

/// Validate signal geometry. Used by both impls; centralized so error
/// shape is identical.
pub(crate) fn validate_geometry(trigger: f64, stop: f64) -> Result<f64> {
    if !trigger.is_finite() || !stop.is_finite() {
        return Err(ExitPolicyError::InvalidGeometry);
    }
    let r = (trigger - stop).abs();
    if r == 0.0 {
        return Err(ExitPolicyError::InvalidGeometry);
    }
    Ok(r)
}

/// Apply a signed multiplier to the entry to produce a target price.
/// `+1` for long, `-1` for short.
pub(crate) fn signed_multiplier(direction: Direction) -> f64 {
    match direction {
        Direction::Long => 1.0,
        Direction::Short => -1.0,
    }
}
