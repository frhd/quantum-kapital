use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsItem {
    pub time_published: DateTime<Utc>,
    pub title: String,
    pub summary: String,
    pub source: String,
    pub url: String,
    pub overall_sentiment_score: Option<f64>,
    pub overall_sentiment_label: Option<String>,
    pub ticker_sentiment: Vec<TickerSentiment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickerSentiment {
    pub ticker: String,
    pub relevance_score: f64,
    pub ticker_sentiment_score: f64,
    pub ticker_sentiment_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewsTone {
    Bullish,
    Bearish,
    Neutral,
}

impl NewsTone {
    #[allow(dead_code)] // Symmetric with `parse`; exposed for future serializers / logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            NewsTone::Bullish => "bullish",
            NewsTone::Bearish => "bearish",
            NewsTone::Neutral => "neutral",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bullish" => Some(NewsTone::Bullish),
            "bearish" => Some(NewsTone::Bearish),
            "neutral" => Some(NewsTone::Neutral),
            _ => None,
        }
    }
}

/// LLM-derived per-symbol news classification. Persisted in
/// `news_cache.news_verdict_json` and consumed by the EP detector to
/// disambiguate sentiment polarity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsVerdict {
    pub tone: NewsTone,
    pub ep_worthy: bool,
    pub parabolic_risk: bool,
    pub summary: String,
}
