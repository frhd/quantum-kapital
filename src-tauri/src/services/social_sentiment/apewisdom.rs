//! Apewisdom provider.
//!
//! Apewisdom (`https://apewisdom.io/api/v1.0/filter/wallstreetbets/page/1`)
//! aggregates Reddit + Stocktwits mentions into a per-ticker rank, raw
//! mention count, sentiment ("Bullish"|"Bearish"|"Neutral"), and a
//! 0–100 sentiment score. We pick the WSB filter for v1; switch to
//! `all` if/when we want a broader pool.
//!
//! Score normalisation: Apewisdom publishes `sentiment_score` as a
//! 0–100 confidence number alongside a categorical `sentiment` label.
//! We map the label to a sign (`Bullish`=+1, `Bearish`=-1, `Neutral`=0)
//! and multiply by `sentiment_score / 100` to land in `[-1, 1]`.

use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

use crate::services::social_sentiment::provider::{HttpFetcher, SentimentProvider};
use crate::services::social_sentiment::types::{
    SentimentLabel, SentimentSample, SentimentSource,
};

pub const APEWISDOM_DEFAULT_URL: &str =
    "https://apewisdom.io/api/v1.0/filter/wallstreetbets/page/1";

#[derive(Debug, Deserialize)]
struct ApewisdomResponse {
    #[serde(default)]
    results: Vec<ApewisdomItem>,
}

#[derive(Debug, Deserialize, Clone)]
struct ApewisdomItem {
    ticker: String,
    #[serde(default)]
    rank: Option<i64>,
    #[serde(default)]
    mentions: Option<i64>,
    #[serde(default)]
    sentiment: Option<String>, // "Bullish" | "Bearish" | "Neutral"
    #[serde(default)]
    sentiment_score: Option<f64>, // 0..=100
}

pub struct ApewisdomProvider {
    http: Arc<dyn HttpFetcher>,
    url: String,
}

impl ApewisdomProvider {
    pub fn new(http: Arc<dyn HttpFetcher>) -> Self {
        Self {
            http,
            url: APEWISDOM_DEFAULT_URL.to_string(),
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    fn parse_label(s: &str) -> Option<SentimentLabel> {
        match s.to_ascii_lowercase().as_str() {
            "bullish" => Some(SentimentLabel::Bullish),
            "bearish" => Some(SentimentLabel::Bearish),
            "neutral" => Some(SentimentLabel::Neutral),
            _ => None,
        }
    }

    fn normalised_score(item: &ApewisdomItem) -> Option<f64> {
        let raw = item.sentiment_score?;
        let label = item.sentiment.as_deref().and_then(Self::parse_label)?;
        let sign = match label {
            SentimentLabel::Bullish => 1.0,
            SentimentLabel::Bearish => -1.0,
            SentimentLabel::Neutral => 0.0,
        };
        // Clamp to defend against upstream returning > 100.
        let mag = (raw / 100.0).clamp(0.0, 1.0);
        Some((sign * mag).clamp(-1.0, 1.0))
    }

    fn item_to_sample(item: ApewisdomItem) -> Result<SentimentSample, String> {
        let raw_payload = serde_json::to_string(&serde_json::json!({
            "ticker": item.ticker,
            "rank": item.rank,
            "mentions": item.mentions,
            "sentiment": item.sentiment,
            "sentiment_score": item.sentiment_score,
        }))
        .map_err(|e| format!("apewisdom raw payload encode: {e}"))?;
        let label = item
            .sentiment
            .as_deref()
            .and_then(ApewisdomProvider::parse_label);
        let score = Self::normalised_score(&item);
        Ok(SentimentSample {
            source: SentimentSource::Apewisdom,
            symbol: item.ticker.to_uppercase(),
            score,
            mentions_24h: item.mentions,
            label,
            rank: item.rank,
            raw_payload,
            is_stale: false,
        })
    }
}

#[async_trait]
impl SentimentProvider for ApewisdomProvider {
    fn id(&self) -> &'static str {
        SentimentSource::Apewisdom.as_str()
    }

    async fn fetch(&self, symbols: &[String]) -> Result<Vec<SentimentSample>, String> {
        let body = self.http.get_text(&self.url, &[]).await?;
        let parsed: ApewisdomResponse = serde_json::from_str(&body)
            .map_err(|e| format!("apewisdom parse: {e}"))?;

        let wanted: std::collections::HashSet<String> = symbols
            .iter()
            .map(|s| s.to_ascii_uppercase())
            .collect();

        let mut out = Vec::new();
        for item in parsed.results {
            if wanted.is_empty() || wanted.contains(&item.ticker.to_ascii_uppercase()) {
                out.push(ApewisdomProvider::item_to_sample(item)?);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::social_sentiment::provider::MockHttpFetcher;

    #[tokio::test]
    async fn parses_apewisdom_response_and_normalises_score() {
        let http = Arc::new(MockHttpFetcher::new());
        http.respond_with(
            "https://test.local/ape",
            r#"{"results":[
                {"ticker":"TSLA","rank":1,"mentions":420,
                 "sentiment":"Bullish","sentiment_score":80.0},
                {"ticker":"GME","rank":2,"mentions":120,
                 "sentiment":"Bearish","sentiment_score":50.0},
                {"ticker":"NOTE","rank":3,"mentions":10,
                 "sentiment":"Neutral","sentiment_score":40.0}
            ]}"#,
        );
        let provider = ApewisdomProvider::new(http).with_url("https://test.local/ape");
        let out = provider
            .fetch(&["TSLA".into(), "GME".into(), "NOTE".into()])
            .await
            .expect("ok");
        assert_eq!(out.len(), 3);

        let tsla = out.iter().find(|s| s.symbol == "TSLA").unwrap();
        assert_eq!(tsla.label, Some(SentimentLabel::Bullish));
        assert!((tsla.score.unwrap() - 0.8).abs() < 1e-9, "0.8 from 80% bullish");
        assert_eq!(tsla.mentions_24h, Some(420));
        assert_eq!(tsla.rank, Some(1));

        let gme = out.iter().find(|s| s.symbol == "GME").unwrap();
        assert!((gme.score.unwrap() + 0.5).abs() < 1e-9, "-0.5 from 50% bearish");

        let note = out.iter().find(|s| s.symbol == "NOTE").unwrap();
        assert_eq!(note.score, Some(0.0), "neutral always 0 regardless of confidence");
    }

    #[tokio::test]
    async fn filters_to_requested_symbols() {
        let http = Arc::new(MockHttpFetcher::new());
        http.respond_with(
            "https://test.local/ape",
            r#"{"results":[
                {"ticker":"TSLA","rank":1,"mentions":1,"sentiment":"Bullish","sentiment_score":50.0},
                {"ticker":"AMD","rank":2,"mentions":1,"sentiment":"Bullish","sentiment_score":50.0}
            ]}"#,
        );
        let provider = ApewisdomProvider::new(http).with_url("https://test.local/ape");
        let out = provider.fetch(&["AMD".into()]).await.expect("ok");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].symbol, "AMD");
    }
}
