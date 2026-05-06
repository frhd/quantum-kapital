//! Phase 8 — value types shared across the portfolio_risk module.

use serde::{Deserialize, Serialize};

/// What kind of concentration limit was breached. Persisted as a
/// short tag on `gate_overrides.gate_kind` and on the
/// `setup.gate_warning` annotation so the UI can color-code without
/// parsing JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcentrationKind {
    /// Total open dollar-risk would exceed `max_total_pct_nlv`.
    TotalRisk,
    /// Single-name dollar-risk would exceed `max_name_pct_nlv`.
    SingleName,
    /// Single-sector dollar-risk would exceed `max_sector_pct_nlv`.
    SingleSector,
    /// Concurrent positions in same factor bucket would exceed
    /// `max_factor_concurrent`.
    FactorConcurrent,
}

impl ConcentrationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConcentrationKind::TotalRisk => "total_risk",
            ConcentrationKind::SingleName => "single_name",
            ConcentrationKind::SingleSector => "single_sector",
            ConcentrationKind::FactorConcurrent => "factor_concurrent",
        }
    }
}

/// Description of a specific gate breach. The `current` /
/// `projected` / `limit` values are in the unit appropriate for
/// `kind` (cents for risk gates, integer count for the factor-
/// concurrent gate).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateLimitBreach {
    pub kind: ConcentrationKind,
    /// Bucket label that breached, e.g. `"tech"` for SingleSector,
    /// `"NVDA"` for SingleName, `"momentum_high"` for
    /// FactorConcurrent. Empty for TotalRisk.
    pub label: String,
    /// Pre-trade exposure in the bucket (cents for risk gates,
    /// count for factor-concurrent).
    pub current: i64,
    /// Post-trade projected exposure (current + candidate's
    /// dollar-risk or +1).
    pub projected: i64,
    /// Configured limit in the same units as `current` / `projected`.
    pub limit: i64,
    /// 0..=2 — `pass`, `warn`, `block`. Persisted alongside so the
    /// frontend can render the right banner color.
    pub severity: u8,
}
