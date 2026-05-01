//! Stocktwits provider.
//!
//! Stocktwits exposes a free per-symbol stream at
//! `https://api.stocktwits.com/api/2/streams/symbol/{SYMBOL}.json`,
//! capped at 30 messages per response. Each message carries an
//! `entities.sentiment.basic` field with `Bullish` / `Bearish` (or
//! absent for neutral). We fetch one stream per symbol and reduce it
//! to a single sample: `score` = (bullish - bearish) / total, and
//! `mentions_24h` = total messages whose `created_at` falls inside
//! the trailing 24h window.
//!
//! Rate limit: 200 req/hour for the free tier — well below what 60min
//! cadence on a small watchlist needs. Per-symbol calls are sequential
//! inside `fetch` to avoid bursting.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::sync::Arc;

use crate::services::social_sentiment::provider::{HttpFetcher, SentimentProvider};
use crate::services::social_sentiment::types::{
    SentimentLabel, SentimentSample, SentimentSource,
};

pub const STOCKTWITS_DEFAULT_BASE: &str = "https://api.stocktwits.com/api/2/streams/symbol";

#[derive(Debug, Deserialize)]
struct StocktwitsResponse {
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize, Clone)]
struct Message {
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    entities: Option<Entities>,
}

#[derive(Debug, Deserialize, Clone)]
struct Entities {
    #[serde(default)]
    sentiment: Option<Sentiment>,
}

#[derive(Debug, Deserialize, Clone)]
struct Sentiment {
    #[serde(default)]
    basic: Option<String>,
}

pub struct StocktwitsProvider {
    http: Arc<dyn HttpFetcher>,
    base_url: String,
}

impl StocktwitsProvider {
    pub fn new(http: Arc<dyn HttpFetcher>) -> Self {
        Self {
            http,
            base_url: STOCKTWITS_DEFAULT_BASE.to_string(),
        }
    }

    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base_url = base.into();
        self
    }

    fn url_for(&self, symbol: &str) -> String {
        format!("{}/{}.json", self.base_url.trim_end_matches('/'), symbol)
    }

    fn reduce(symbol: &str, body: &str, now: DateTime<Utc>) -> Result<SentimentSample, String> {
        let parsed: StocktwitsResponse = serde_json::from_str(body)
            .map_err(|e| format!("stocktwits parse: {e}"))?;

        let cutoff = now - Duration::hours(24);
        let mut bullish = 0_i64;
        let mut bearish = 0_i64;
        let mut counted = 0_i64;

        for msg in &parsed.messages {
            let in_window = match msg.created_at.as_deref() {
                Some(ts) => DateTime::parse_from_rfc3339(ts)
                    .map(|dt| dt.with_timezone(&Utc) >= cutoff)
                    .unwrap_or(false),
                None => false,
            };
            if !in_window {
                continue;
            }
            counted += 1;
            match msg
                .entities
                .as_ref()
                .and_then(|e| e.sentiment.as_ref())
                .and_then(|s| s.basic.as_deref())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("bullish") => bullish += 1,
                Some("bearish") => bearish += 1,
                _ => {}
            }
        }

        let raw_payload = serde_json::to_string(&serde_json::json!({
            "messages_in_window": counted,
            "bullish": bullish,
            "bearish": bearish,
        }))
        .map_err(|e| format!("stocktwits payload encode: {e}"))?;

        if counted == 0 {
            return Ok(SentimentSample {
                source: SentimentSource::Stocktwits,
                symbol: symbol.to_uppercase(),
                score: None,
                mentions_24h: Some(0),
                label: None,
                rank: None,
                raw_payload,
                is_stale: true,
            });
        }

        let score = (bullish - bearish) as f64 / counted as f64;
        let label = if bullish > bearish {
            SentimentLabel::Bullish
        } else if bearish > bullish {
            SentimentLabel::Bearish
        } else {
            SentimentLabel::Neutral
        };
        Ok(SentimentSample {
            source: SentimentSource::Stocktwits,
            symbol: symbol.to_uppercase(),
            score: Some(score.clamp(-1.0, 1.0)),
            mentions_24h: Some(counted),
            label: Some(label),
            rank: None,
            raw_payload,
            is_stale: false,
        })
    }
}

#[async_trait]
impl SentimentProvider for StocktwitsProvider {
    fn id(&self) -> &'static str {
        SentimentSource::Stocktwits.as_str()
    }

    async fn fetch(&self, symbols: &[String]) -> Result<Vec<SentimentSample>, String> {
        let now = Utc::now();
        let mut out = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            let url = self.url_for(symbol);
            match self.http.get_text(&url, &[]).await {
                Ok(body) => match Self::reduce(symbol, &body, now) {
                    Ok(sample) => out.push(sample),
                    Err(e) => {
                        tracing::warn!("stocktwits reduce failed for {symbol}: {e}");
                        out.push(stale_sample(symbol));
                    }
                },
                Err(e) => {
                    tracing::warn!("stocktwits fetch failed for {symbol}: {e}");
                    out.push(stale_sample(symbol));
                }
            }
        }
        Ok(out)
    }
}

fn stale_sample(symbol: &str) -> SentimentSample {
    SentimentSample {
        source: SentimentSource::Stocktwits,
        symbol: symbol.to_uppercase(),
        score: None,
        mentions_24h: None,
        label: None,
        rank: None,
        raw_payload: "{}".to_string(),
        is_stale: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::social_sentiment::provider::MockHttpFetcher;

    fn iso(secs_ago: i64) -> String {
        (Utc::now() - Duration::seconds(secs_ago)).to_rfc3339()
    }

    #[tokio::test]
    async fn reduces_bullish_majority_into_positive_score() {
        let http = Arc::new(MockHttpFetcher::new());
        let body = format!(
            r#"{{"messages":[
                {{"created_at":"{a}","entities":{{"sentiment":{{"basic":"Bullish"}}}}}},
                {{"created_at":"{b}","entities":{{"sentiment":{{"basic":"Bullish"}}}}}},
                {{"created_at":"{c}","entities":{{"sentiment":{{"basic":"Bearish"}}}}}},
                {{"created_at":"{d}"}}
            ]}}"#,
            a = iso(60),
            b = iso(120),
            c = iso(180),
            d = iso(240),
        );
        http.respond_with("https://test.local/twits/TSLA.json", &body);
        let provider = StocktwitsProvider::new(http).with_base_url("https://test.local/twits");
        let out = provider.fetch(&["TSLA".into()]).await.expect("ok");
        assert_eq!(out.len(), 1);
        let s = &out[0];
        assert_eq!(s.symbol, "TSLA");
        assert_eq!(s.mentions_24h, Some(4));
        // (2 bull - 1 bear) / 4 = 0.25
        assert!((s.score.unwrap() - 0.25).abs() < 1e-9);
        assert_eq!(s.label, Some(SentimentLabel::Bullish));
    }

    #[tokio::test]
    async fn ignores_messages_outside_24h_window() {
        let http = Arc::new(MockHttpFetcher::new());
        let stale = (Utc::now() - Duration::hours(48)).to_rfc3339();
        let body = format!(
            r#"{{"messages":[
                {{"created_at":"{stale}","entities":{{"sentiment":{{"basic":"Bullish"}}}}}}
            ]}}"#
        );
        http.respond_with("https://test.local/twits/AMD.json", &body);
        let provider = StocktwitsProvider::new(http).with_base_url("https://test.local/twits");
        let out = provider.fetch(&["AMD".into()]).await.expect("ok");
        assert_eq!(out.len(), 1);
        assert!(out[0].is_stale, "no messages in window -> stale row");
        assert_eq!(out[0].mentions_24h, Some(0));
    }

    #[tokio::test]
    async fn fetch_failure_produces_stale_row_per_symbol() {
        let http = Arc::new(MockHttpFetcher::new());
        // No `respond_with` -> MockHttpFetcher returns Err.
        let provider = StocktwitsProvider::new(http).with_base_url("https://test.local/twits");
        let out = provider.fetch(&["XYZ".into()]).await.expect("ok");
        assert_eq!(out.len(), 1);
        assert!(out[0].is_stale);
        assert_eq!(out[0].symbol, "XYZ");
    }
}
