//! Wire types for the trader profile aggregator.
//!
//! `TraderProfile` is the persisted-shape envelope returned by the
//! `get_trader_profile` MCP read tool and consumed by the morning_sweep
//! playbook step. All fields are aggregated server-side from
//! `day_reviews`; no LLM, no IBKR.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::services::trade_reviews::tags::BehavioralTag;

/// A single tag's frequency over the window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TagFrequency {
    pub tag: BehavioralTag,
    pub count: i64,
    pub pct_of_reviews: f64,
}

/// Aggregate P&L attributable to days that fired this tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PnlByTag {
    pub tag: BehavioralTag,
    pub n_days: i64,
    pub net_pnl_total: f64,
    pub net_pnl_per_day_avg: f64,
}

/// One window of the trendline (e.g. last_7d or prior_21d).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WindowSummary {
    pub n_reviews: i64,
    pub tag_counts: BTreeMap<String, i64>,
    pub net_pnl: f64,
    pub avg_grade_score: f64,
}

/// Two-window trend split — last 7d vs the 21d preceding it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Trendline {
    pub last_7d: WindowSummary,
    pub prior_21d: WindowSummary,
}

/// A specific recent leg observation that fired a tag, surfaced into the
/// playbook prompt so the LLM can name the symbol when adding it to the
/// skip list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecentIncident {
    pub date: NaiveDate,
    pub symbol: String,
    pub tag: BehavioralTag,
    pub leg_observation: String,
}

/// Persisted-shape envelope returned by `get_trader_profile`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraderProfile {
    pub account: String,
    pub window_days: u32,
    pub since_date: NaiveDate,
    pub n_reviews: i64,
    pub tag_frequencies: Vec<TagFrequency>,
    pub pnl_by_tag: Vec<PnlByTag>,
    pub trendline: Trendline,
    pub recent_incidents: Vec<RecentIncident>,
}
