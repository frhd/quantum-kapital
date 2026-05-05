//! Phase 1 — `services/risk_engine/` value types.
//!
//! `Sizing` is the engine's output: a deterministic position size for
//! a given setup against a pinned equity snapshot. Stored on the
//! `setups` row (cents-encoded) so a future "resize this row"
//! operation can replay deterministically by reading the same
//! snapshot + config + version.
//!
//! `RiskConfig` lives in `AppConfig.risk_engine` and round-trips
//! through `get_settings` / `update_settings`. Defaults match the
//! master plan's `Defaults committed` table — A=0.50%, B=0.33%,
//! C=0.16%, max_position_pct=0.25, min_dollar_risk=$10.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// `Sizing` schema revision. Bump when `compute_sizing` changes
/// its formula in a way that would invalidate replayability against
/// stored rows. P4 grading reads `sizing_version` so older rows
/// stay attributable to the formula they were sized under.
pub const SIZING_VERSION: i32 = 1;

/// Half-Kelly-ish per-trade risk by conviction grade. A continuous
/// `conviction_signal: f64` from the detector maps to one of these
/// via [`ConvictionGrade::from_signal`]; the LLM thesis (P17) can
/// later override on a per-setup basis but P1 sizes from the
/// detector signal alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum ConvictionGrade {
    A,
    B,
    C,
}

impl ConvictionGrade {
    /// Map a continuous detector signal in `[0.0, 1.0]` to a discrete
    /// grade. Thresholds are intentionally conservative: only signals
    /// at or above 0.75 graduate to A. Below 0.5 collapses to C, which
    /// keeps an unconfident detector hit small until the LLM thesis
    /// (or a calibrated detector — see master `Defaults committed`)
    /// can lift it.
    pub fn from_signal(signal: f64) -> Self {
        if !signal.is_finite() {
            return ConvictionGrade::C;
        }
        if signal >= 0.75 {
            ConvictionGrade::A
        } else if signal >= 0.5 {
            ConvictionGrade::B
        } else {
            ConvictionGrade::C
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ConvictionGrade::A => "A",
            ConvictionGrade::B => "B",
            ConvictionGrade::C => "C",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "A" => Some(ConvictionGrade::A),
            "B" => Some(ConvictionGrade::B),
            "C" => Some(ConvictionGrade::C),
            _ => None,
        }
    }
}

/// Origin of the equity number the engine pinned sizing to. Used so
/// the UI can render an "ungated" warning when the snapshot is
/// older than one trading day or sourced from a stale-cache fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquitySource {
    /// Fresh fetch from `IbkrClient::get_account_summary` via the
    /// `EquityFetcher` seam.
    IbkrAccountSummary,
    /// Most-recent persisted row used because IBKR was unreachable
    /// at decision time. Sizing still happens; the UI shows a
    /// stale-snapshot banner.
    StaleCache,
    /// Manually entered by the trader (recover-from-disconnect path).
    /// Reserved — no command persists this value in P1.
    Manual,
}

impl EquitySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            EquitySource::IbkrAccountSummary => "ibkr_account_summary",
            EquitySource::StaleCache => "stale_cache",
            EquitySource::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ibkr_account_summary" => Some(EquitySource::IbkrAccountSummary),
            "stale_cache" => Some(EquitySource::StaleCache),
            "manual" => Some(EquitySource::Manual),
            _ => None,
        }
    }
}

/// One row in `equity_snapshots`. NLV stays in integer cents to
/// dodge f64-through-SQLite drift; convert at the API edge with
/// [`EquitySnapshot::nlv`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquitySnapshot {
    pub account: String,
    /// ET trading-day the snapshot pertains to as ISO `YYYY-MM-DD`.
    /// Sizing decisions made on the same trading day all read the
    /// same row, so two consecutive setups against the same NLV
    /// produce identical dollar-risk numbers.
    pub as_of_date: String,
    pub nlv_cents: i64,
    pub source: EquitySource,
    pub fetched_at: DateTime<Utc>,
}

impl EquitySnapshot {
    pub fn nlv(&self) -> f64 {
        self.nlv_cents as f64 / 100.0
    }
}

/// Why the engine refused to size this setup. Persisted on the
/// row's `sizing_skipped_reason` so the UI can render an explicit
/// "skipped: below_min_risk" badge instead of a phantom zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizingSkippedReason {
    /// `r_per_share` is zero or non-finite (defensive — detectors
    /// shouldn't emit this, but the engine refuses to divide by
    /// zero).
    ZeroR,
    /// `target_dollar_risk` is below `RiskConfig.min_dollar_risk`,
    /// or the cap-applied qty rounds to zero.
    BelowMinRisk,
    /// Detected setup outside trading hours / no fresh equity row;
    /// engine fell back to a snapshot older than the configured
    /// staleness budget. Reserved — not produced by P1 default
    /// policy.
    StaleSnapshot,
    /// Reserved for P11 tilt-guard. P1 does not emit this; the
    /// variant exists so the schema is forward-compatible.
    TiltPaused,
    /// Trigger or stop is non-finite or non-positive.
    InvalidPrice,
}

impl SizingSkippedReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            SizingSkippedReason::ZeroR => "zero_r",
            SizingSkippedReason::BelowMinRisk => "below_min_risk",
            SizingSkippedReason::StaleSnapshot => "stale_snapshot",
            SizingSkippedReason::TiltPaused => "tilt_paused",
            SizingSkippedReason::InvalidPrice => "invalid_price",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "zero_r" => Some(SizingSkippedReason::ZeroR),
            "below_min_risk" => Some(SizingSkippedReason::BelowMinRisk),
            "stale_snapshot" => Some(SizingSkippedReason::StaleSnapshot),
            "tilt_paused" => Some(SizingSkippedReason::TiltPaused),
            "invalid_price" => Some(SizingSkippedReason::InvalidPrice),
            _ => None,
        }
    }
}

/// Knobs the user / settings file can tune. Defaults match the
/// master plan; the field set is locked until P4 calibration shows
/// conviction multiplier > 1.0 is justified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Fraction of equity risked per A-conviction trade. 0.005 = 0.50%.
    pub risk_pct_a: f64,
    /// Fraction risked per B-conviction trade. 0.0033 ≈ 0.33%.
    pub risk_pct_b: f64,
    /// Fraction risked per C-conviction trade. 0.0016 ≈ 0.16%.
    pub risk_pct_c: f64,
    /// Hard cap on notional position as a fraction of equity. Binds
    /// when low-vol stocks blow past dollar-risk math. Default 0.25.
    pub max_position_pct: f64,
    /// Floor below which the engine refuses to size. Commission
    /// noise dominates pnl below ~$10 risk for retail. Default $10.
    pub min_dollar_risk: f64,
    /// Multiplier applied on top of the per-grade `risk_pct`.
    /// Capped at 1.0 in P1 — see master "Decisions to make in this
    /// phase". P4 calibration unlocks > 1.0.
    pub conviction_multiplier_cap: f64,
    /// Round qty down to a multiple of this value. 1 = nearest
    /// whole share (P1 default). 100 = lot pricing.
    pub round_lot: u32,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            risk_pct_a: 0.005,
            risk_pct_b: 0.0033,
            risk_pct_c: 0.0016,
            max_position_pct: 0.25,
            min_dollar_risk: 10.0,
            conviction_multiplier_cap: 1.0,
            round_lot: 1,
        }
    }
}

impl RiskConfig {
    pub fn risk_pct_for(&self, grade: ConvictionGrade) -> f64 {
        match grade {
            ConvictionGrade::A => self.risk_pct_a,
            ConvictionGrade::B => self.risk_pct_b,
            ConvictionGrade::C => self.risk_pct_c,
        }
    }
}

/// Output of `compute_sizing`. Lands on the `setups` row via
/// `TrackerService::update_setup_sizing`. Cent-encoded fields keep
/// the SQLite schema integer-only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sizing {
    /// Whole-share quantity. Zero only when `skipped_reason` is
    /// `Some(_)` — a non-skipped sizing always emits at least one
    /// share.
    pub qty: u32,
    pub dollar_risk_cents: i64,
    pub r_per_share_cents: i64,
    pub equity_at_decision_cents: i64,
    pub conviction_grade: ConvictionGrade,
    /// Multiplier applied beyond the per-grade `risk_pct`, in basis
    /// points. 10000 = 1.0×. Capped at `conviction_multiplier_cap *
    /// 10000` so the field can't carry an unvetted scale.
    pub conviction_multiplier_bps: u32,
    /// True when `max_position_pct` clipped the qty below what the
    /// dollar-risk math would have produced. Useful diagnostic for
    /// "why did sizing pick fewer shares than I expected".
    pub cap_applied: bool,
    /// `Some(_)` when the engine refused to size; `qty` is then 0.
    pub skipped_reason: Option<SizingSkippedReason>,
    pub version: i32,
}

impl Sizing {
    /// Convenience constructor for skipped sizings — qty/risk default
    /// to zero, version pinned to current. Used by both the engine
    /// and tests; keeps the "skipped" shape trivially comparable.
    pub fn skipped(
        reason: SizingSkippedReason,
        equity_cents: i64,
        grade: ConvictionGrade,
    ) -> Self {
        Self {
            qty: 0,
            dollar_risk_cents: 0,
            r_per_share_cents: 0,
            equity_at_decision_cents: equity_cents,
            conviction_grade: grade,
            conviction_multiplier_bps: 0,
            cap_applied: false,
            skipped_reason: Some(reason),
            version: SIZING_VERSION,
        }
    }

    pub fn is_skipped(&self) -> bool {
        self.skipped_reason.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_signal_thresholds() {
        assert_eq!(ConvictionGrade::from_signal(0.9), ConvictionGrade::A);
        assert_eq!(ConvictionGrade::from_signal(0.75), ConvictionGrade::A);
        assert_eq!(ConvictionGrade::from_signal(0.74), ConvictionGrade::B);
        assert_eq!(ConvictionGrade::from_signal(0.5), ConvictionGrade::B);
        assert_eq!(ConvictionGrade::from_signal(0.49), ConvictionGrade::C);
        assert_eq!(ConvictionGrade::from_signal(0.0), ConvictionGrade::C);
    }

    #[test]
    fn from_signal_nan_falls_back_to_c() {
        // Defensive — a detector emitting NaN shouldn't crash the engine
        // or sneak through as A.
        assert_eq!(ConvictionGrade::from_signal(f64::NAN), ConvictionGrade::C);
        assert_eq!(
            ConvictionGrade::from_signal(f64::INFINITY),
            ConvictionGrade::C
        );
    }

    #[test]
    fn risk_pct_for_grade_matches_committed_defaults() {
        let cfg = RiskConfig::default();
        assert_eq!(cfg.risk_pct_for(ConvictionGrade::A), 0.005);
        assert_eq!(cfg.risk_pct_for(ConvictionGrade::B), 0.0033);
        assert_eq!(cfg.risk_pct_for(ConvictionGrade::C), 0.0016);
        assert_eq!(cfg.max_position_pct, 0.25);
        assert_eq!(cfg.min_dollar_risk, 10.0);
        assert_eq!(cfg.conviction_multiplier_cap, 1.0);
    }

    #[test]
    fn equity_snapshot_nlv_round_trips_cents() {
        let snap = EquitySnapshot {
            account: "DU123".to_string(),
            as_of_date: "2026-05-04".to_string(),
            nlv_cents: 12_345_678,
            source: EquitySource::IbkrAccountSummary,
            fetched_at: Utc::now(),
        };
        assert!((snap.nlv() - 123_456.78).abs() < 1e-9);
    }

    #[test]
    fn skipped_sizing_carries_zero_qty_and_reason() {
        let s = Sizing::skipped(
            SizingSkippedReason::BelowMinRisk,
            10_000_000,
            ConvictionGrade::C,
        );
        assert!(s.is_skipped());
        assert_eq!(s.qty, 0);
        assert_eq!(s.dollar_risk_cents, 0);
        assert_eq!(
            s.skipped_reason,
            Some(SizingSkippedReason::BelowMinRisk)
        );
        assert_eq!(s.version, SIZING_VERSION);
    }
}
