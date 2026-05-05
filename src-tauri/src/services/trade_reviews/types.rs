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

use super::grade::{Grade, GradeLetter};
use super::tags::BehavioralTag;

/// Per-leg observation surfaced into the review's `leg_observations`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegObservation {
    pub leg_id: String,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeReview {
    pub date: NaiveDate,
    pub account: String,
    pub prompt_version: i32,
    pub generated_at: DateTime<Utc>,
    pub grade: GradeLetter,
    pub grade_score: f64,
    pub summary: LegSummary,
    pub behavioral_tags: Vec<BehavioralTag>,
    pub leg_observations: Vec<LegObservation>,
    pub narrative_md: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_call_id: Option<String>,
}

impl TradeReview {
    /// Convenience: re-pack `(grade, grade_score)` as a [`Grade`] struct.
    pub fn computed_grade(&self) -> Grade {
        Grade {
            grade: self.grade,
            score: self.grade_score,
        }
    }
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
