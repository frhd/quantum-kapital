use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ibkr::types::{BarSize, StrategyTag};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Long,
    Short,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetLevel {
    pub label: String,
    pub price: f64,
}

/// Phase 5 — short tag identifying *why* a setup was skipped (rather
/// than fired). Persisted on the `setups` row as `skipped_reason` and
/// surfaced to the UI so the trader can review skipped detector hits in
/// the SkippedSetupsPanel. Distinct from
/// [`crate::services::risk_engine::SizingSkippedReason`], which tracks
/// risk-engine sizing failures on a fired setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Detector hit fell inside an earnings blackout window for this
    /// detector's per-strategy policy.
    EarningsBlackout,
    /// Detector hit fell inside the FOMC day-of blackout window.
    FomcBlackout,
    /// Phase 8 — concentration gate refused the candidate. The
    /// `skip_window_json` carries the breach descriptor (`kind`,
    /// `limit`, `current`, `delta`).
    ConcentrationBlocked,
    /// Phase 9 — regime gate refused the candidate. The
    /// `skip_window_json` carries the active regime + the
    /// detector's declared preferred-regime filter, so the panel can
    /// render "skipped: trend=Down vol=High; preferred trend in
    /// [Up,Sideways] && vol in [Low,Normal]" without re-deriving the
    /// gate decision.
    OffRegime,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::EarningsBlackout => "earnings_blackout",
            SkipReason::FomcBlackout => "fomc_blackout",
            SkipReason::ConcentrationBlocked => "concentration_blocked",
            SkipReason::OffRegime => "off_regime",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "earnings_blackout" => Some(SkipReason::EarningsBlackout),
            "fomc_blackout" => Some(SkipReason::FomcBlackout),
            "concentration_blocked" => Some(SkipReason::ConcentrationBlocked),
            "off_regime" => Some(SkipReason::OffRegime),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupCandidate {
    pub strategy: &'static str,
    pub tag: StrategyTag,
    pub direction: Direction,
    pub conviction_signal: f64,
    pub trigger_price: f64,
    pub stop_price: f64,
    pub targets: Vec<TargetLevel>,
    pub raw_signals: serde_json::Value,
    pub timeframe: BarSize,
    pub detected_at: DateTime<Utc>,
}

pub fn targets_for_risk_profile(
    direction: Direction,
    trigger: f64,
    stop: f64,
) -> Result<Vec<TargetLevel>, &'static str> {
    if !trigger.is_finite() || !stop.is_finite() {
        return Err("trigger and stop must be finite");
    }
    let risk = (trigger - stop).abs();
    if risk == 0.0 {
        return Err("trigger and stop are equal — risk distance is zero");
    }
    let signed = match direction {
        Direction::Long => 1.0,
        Direction::Short => -1.0,
    };
    Ok(vec![
        TargetLevel {
            label: "2R".to_string(),
            price: trigger + signed * 2.0 * risk,
        },
        TargetLevel {
            label: "3R".to_string(),
            price: trigger + signed * 3.0 * risk,
        },
    ])
}
