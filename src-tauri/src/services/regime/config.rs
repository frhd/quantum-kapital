//! Phase 9 — `RegimeConfig`. Persisted to `settings.json` under the
//! `regime` key. Hot-reloadable via `RegimeService::set_config` so an
//! operator can widen or narrow per-detector preferences without a
//! restart.
//!
//! Defaults match the master plan's "Defaults committed" table for the
//! three live detectors. Pre-P9 settings.json files predate the field;
//! `#[serde(default)]` on `AppConfig.regime` keeps them parseable.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::types::{BreadthAxis, CorrAxis};
use super::types::{RegimeFilter, TrendAxis, VolAxis};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeConfig {
    /// `true` enables the gate. When `false`, the runner's regime
    /// check is a no-op (every detector fires regardless of regime).
    /// Defaults to `true` since the master plan committed phase-9 as
    /// load-bearing for sizing/scheduling decisions; operators can
    /// flip off per-installation if the bars_cache is too sparse to
    /// classify reliably.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Minimum monthly trade count per detector under regime gating.
    /// If a P10 walk-forward refit shows a detector dropping below
    /// this floor over a 12-month window, the operator must widen the
    /// preference set or retire the detector. Phase 10 owns the cron
    /// that checks this; Phase 9 only persists the knob.
    #[serde(default = "default_min_monthly_trades")]
    pub min_monthly_trades_floor: u32,

    /// Per-detector preferred regimes keyed by `StrategyDetector::name`.
    /// Detectors not in this map fall back to their `preferred_regimes`
    /// trait default (`RegimeFilter::default()` = "any regime"). When
    /// `enabled = false` the entire map is ignored.
    #[serde(default)]
    pub per_detector: HashMap<String, RegimeFilter>,
}

fn default_enabled() -> bool {
    true
}

fn default_min_monthly_trades() -> u32 {
    5
}

impl Default for RegimeConfig {
    fn default() -> Self {
        let mut per_detector = HashMap::new();

        // Breakout: trend in {Up, Sideways} AND vol in {Low, Normal}.
        // Skip in Down + High vol.
        per_detector.insert(
            "breakout".to_string(),
            RegimeFilter {
                trend: vec![TrendAxis::Up, TrendAxis::Sideways],
                vol: vec![VolAxis::Low, VolAxis::Normal],
                breadth: vec![],
                corr: vec![],
            },
        );

        // Parabolic short: vol in {Normal, High}. Skip in clean
        // melt-ups (Up + Low vol).
        per_detector.insert(
            "parabolic_short".to_string(),
            RegimeFilter {
                trend: vec![TrendAxis::Sideways, TrendAxis::Down],
                vol: vec![VolAxis::Normal, VolAxis::High],
                breadth: vec![],
                corr: vec![],
            },
        );

        // Episodic pivot: regime-agnostic. Empty filter = "any regime".
        per_detector.insert("episodic_pivot".to_string(), RegimeFilter::default());

        Self {
            enabled: default_enabled(),
            min_monthly_trades_floor: default_min_monthly_trades(),
            per_detector,
        }
    }
}

impl RegimeConfig {
    /// Lookup the filter for `detector_name`. Falls through to the
    /// trait default (no constraints) if the operator hasn't declared
    /// preferences for this detector.
    pub fn filter_for(&self, detector_name: &str) -> RegimeFilter {
        self.per_detector
            .get(detector_name)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::regime::types::Regime;

    #[test]
    fn defaults_match_phase_9_table() {
        let cfg = RegimeConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.min_monthly_trades_floor, 5);

        // Breakout default: skipped in Down trend.
        let breakout = cfg.filter_for("breakout");
        let down_high_vol = Regime {
            trend: TrendAxis::Down,
            vol: VolAxis::High,
            breadth: BreadthAxis::Mixed,
            corr: CorrAxis::Mixed,
        };
        assert!(!breakout.matches(&down_high_vol));
        let up_normal = Regime {
            trend: TrendAxis::Up,
            vol: VolAxis::Normal,
            breadth: BreadthAxis::Healthy,
            corr: CorrAxis::Low,
        };
        assert!(breakout.matches(&up_normal));

        // Parabolic short default: skipped in Up + Low vol melt-up.
        let pshort = cfg.filter_for("parabolic_short");
        let up_low_vol = Regime {
            trend: TrendAxis::Up,
            vol: VolAxis::Low,
            breadth: BreadthAxis::Healthy,
            corr: CorrAxis::Low,
        };
        assert!(!pshort.matches(&up_low_vol));
        let sideways_high_vol = Regime {
            trend: TrendAxis::Sideways,
            vol: VolAxis::High,
            breadth: BreadthAxis::Mixed,
            corr: CorrAxis::High,
        };
        assert!(pshort.matches(&sideways_high_vol));

        // Episodic pivot default: regime-agnostic.
        let episodic = cfg.filter_for("episodic_pivot");
        assert!(episodic.matches(&down_high_vol));
        assert!(episodic.matches(&up_normal));
    }

    #[test]
    fn unknown_detector_falls_through_to_default() {
        let cfg = RegimeConfig::default();
        let f = cfg.filter_for("not_a_real_detector");
        assert_eq!(f, RegimeFilter::default());
    }
}
