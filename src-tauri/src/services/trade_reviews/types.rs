//! Wire types for the trade-review subsystem.
//!
//! `LegSummary` is the pre-computed numerical input to `compute_grade`
//! and the LLM prompt; `TradeReview` is the persisted artifact returned
//! by `get_trade_review`. The agent supplies a `WriteTradeReviewRequest`
//! carrying tags + narrative — the server computes the grade.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::equity_curve::EquityPoint;
use super::grade::GradeLetter;
use super::risk_metrics::RiskMetrics;
use super::tags::BehavioralTag;

/// Per-leg observation surfaced into the review's `leg_observations`.
///
/// `symbol` is optional for backward compatibility with rows authored
/// before Phase 6, but the EOD review writer is expected to populate it
/// going forward so the Phase 6 trader-profile aggregator can name the
/// instrument in `RecentIncident`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegObservation {
    pub leg_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub observation_md: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<BehavioralTag>,
}

/// Pre-computed numerical summary of a day's legs. Input to `compute_grade`
/// and to the agent's prompt. The agent does NOT recompute these — they
/// are the trusted server-side numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegSummary {
    pub gross_pnl: f64,
    pub net_pnl: f64,
    pub commissions_total: f64,
    pub n_round_trips: usize,
    pub n_carryover: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub win_rate: Option<f64>,
    #[serde(default)]
    pub by_symbol: BTreeMap<String, f64>,
}

/// Persisted form of a structured trade review.
///
/// Phase 4 split: pre-P4 rows carry `formula_version="v1"` and the
/// legacy `(grade, grade_score)` tuple; new rows carry
/// `formula_version="v2"` plus `score_v2` / `discipline_v2` /
/// `risk_metrics` / `equity_curve` and leave the v1 fields `None`.
/// The two are surfaced separately, never summed (master commitment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeReview {
    pub date: NaiveDate,
    pub account: String,
    pub prompt_version: i32,
    pub generated_at: DateTime<Utc>,
    pub formula_version: String,
    /// Pre-P4 legacy grade letter. `None` for v2 rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<GradeLetter>,
    /// Pre-P4 legacy score (the offending `net_pnl/100 + tags`).
    /// `None` for v2 rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade_score: Option<f64>,
    /// V2: Σ(realized_R × conviction_weight). `None` for v1 rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_v2: Option<f64>,
    /// V2: Σ(tag_weights). Surfaced separately. `None` for v1 rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discipline_v2: Option<f64>,
    /// V2: Sharpe / Sortino / Calmar / PF / expectancy / DD. `None`
    /// for v1 rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_metrics: Option<RiskMetrics>,
    /// V2: daily equity series rendered against this review's date
    /// range. `None` for v1 rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equity_curve: Option<Vec<EquityPoint>>,
    pub summary: LegSummary,
    pub behavioral_tags: Vec<BehavioralTag>,
    pub leg_observations: Vec<LegObservation>,
    pub narrative_md: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_call_id: Option<String>,
}

/// Inputs the MCP write rail accepts. Note the absence of `grade` — the
/// server computes it deterministically from `(summary, behavioral_tags)`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct WriteTradeReviewRequest {
    pub date: NaiveDate,
    pub account: String,
    pub prompt_version: i32,
    pub summary: LegSummary,
    #[serde(default)]
    pub behavioral_tags: Vec<BehavioralTag>,
    #[serde(default)]
    pub leg_observations: Vec<LegObservation>,
    pub narrative_md: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_call_id: Option<String>,
}

/// V2 fields the store writes alongside the legacy summary. Computed
/// by the trade-review generator (or the MCP `write_trade_review` tool)
/// before calling [`super::store::TradeReviewStore::write`]. Pre-P4
/// callers that haven't migrated can pass [`ReviewV2Fields::v1_only`]
/// to opt out — that path stays read-back-compatible (the v1 columns
/// continue to be filled and the row is tagged `formula_version="v1"`).
#[derive(Debug, Clone)]
pub struct ReviewV2Fields {
    pub score_v2: Option<f64>,
    pub discipline_v2: Option<f64>,
    pub risk_metrics: Option<RiskMetrics>,
    pub equity_curve: Option<Vec<EquityPoint>>,
    pub formula_version: String,
}

impl ReviewV2Fields {
    /// Pre-P4 / legacy passthrough. The row stays on `formula_version
    /// = "v1"` and v2 numerics are NULL.
    pub fn v1_only() -> Self {
        Self {
            score_v2: None,
            discipline_v2: None,
            risk_metrics: None,
            equity_curve: None,
            formula_version: "v1".into(),
        }
    }
}
