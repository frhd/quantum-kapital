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
