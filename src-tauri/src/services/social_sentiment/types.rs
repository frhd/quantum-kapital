//! Shared types for the social-sentiment subsystem.

use serde::{Deserialize, Serialize};

/// One source-specific sentiment sample. Returned by every
/// [`SentimentProvider`](super::provider::SentimentProvider) and
/// persisted as a row of `social_sentiment`.
///
/// Score normalisation: every provider that surfaces a polarity must
/// normalise it to `[-1.0, 1.0]` (negative = bearish, positive =
/// bullish, NULL = no score). Mention/post counts go in `mentions_24h`,
/// source-specific rank goes in `rank`. The original provider payload
/// is preserved verbatim in `raw_payload` so we can re-derive scores
/// later if our normalisation drifts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SentimentSample {
    pub source: SentimentSource,
    pub symbol: String,
    pub score: Option<f64>,
    pub mentions_24h: Option<i64>,
    pub label: Option<SentimentLabel>,
    pub rank: Option<i64>,
    /// Raw upstream JSON. For stale rows this is `{}`.
    pub raw_payload: String,
    /// `true` when the provider answered but had no signal for `symbol`
    /// — distinct from "we never asked" (no row at all). Helps the
    /// agent reason about source coverage gaps.
    pub is_stale: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SentimentSource {
    RedditWsb,
    Stocktwits,
    Apewisdom,
}

impl SentimentSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            SentimentSource::RedditWsb => "reddit_wsb",
            SentimentSource::Stocktwits => "stocktwits",
            SentimentSource::Apewisdom => "apewisdom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "reddit_wsb" => Some(SentimentSource::RedditWsb),
            "stocktwits" => Some(SentimentSource::Stocktwits),
            "apewisdom" => Some(SentimentSource::Apewisdom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SentimentLabel {
    Bullish,
    Bearish,
    Neutral,
}

impl SentimentLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            SentimentLabel::Bullish => "bullish",
            SentimentLabel::Bearish => "bearish",
            SentimentLabel::Neutral => "neutral",
        }
    }
}

/// One persisted row, returned by repo queries and the `get_sentiment`
/// MCP tool / Tauri command. `id` and `fetched_at` are populated on
/// read; the writer fills them implicitly via `INSERT`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocialSentimentRow {
    pub id: i64,
    pub source: String,
    pub symbol: String,
    pub score: Option<f64>,
    pub mentions_24h: Option<i64>,
    pub sentiment_label: Option<String>,
    pub rank: Option<i64>,
    pub raw_payload: String,
    pub is_stale: bool,
    pub fetched_at: i64,
}

/// Tool / command response shape: rows grouped by source so the agent
/// gets a single per-source summary rather than a flat history dump.
/// `latest` is the freshest row from each source; `samples` is the
/// time-series within the requested window for charts/audit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceSummary {
    pub source: String,
    pub latest: Option<SocialSentimentRow>,
    pub samples: Vec<SocialSentimentRow>,
}
