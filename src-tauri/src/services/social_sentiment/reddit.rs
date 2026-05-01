//! Reddit (r/wallstreetbets) provider — public-JSON, no OAuth.
//!
//! v1 hits Reddit's public `https://www.reddit.com/r/wallstreetbets/new.json?limit=100`
//! endpoint. The JSON wrapper is `{ data: { children: [{ data: Post }, ...] } }`.
//! No auth required; the only contract is a non-default `User-Agent`.
//!
//! For each requested symbol we count how many post titles + selftexts
//! mention `$SYMBOL` (cashtag) or the bare uppercase token (when it
//! passes [`ticker_filter::is_valid_ticker`]). Sentiment polarity is
//! NOT inferred from raw Reddit text — we only surface mention counts.
//! `score` stays `None`; `label` derives from the count being `> 0`
//! (`Bullish` is too strong an inference, so we leave it `None`).
//!
//! This bypasses the Reddit OAuth flakiness called out in the master
//! plan's open risks. If we later need authed coverage (ranked listings,
//! comment-level scrape, > 100/req throughput) we can swap in a `roux`
//! or PRAW backend behind the same `SentimentProvider` trait.

use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

use crate::services::social_sentiment::provider::{HttpFetcher, SentimentProvider};
use crate::services::social_sentiment::ticker_filter::{extract_valid_tickers, TickerFilterConfig};
use crate::services::social_sentiment::types::{SentimentSample, SentimentSource};

pub const REDDIT_DEFAULT_URL: &str = "https://www.reddit.com/r/wallstreetbets/new.json?limit=100";

#[derive(Debug, Deserialize)]
struct RedditListing {
    #[serde(default)]
    data: ListingData,
}

#[derive(Debug, Deserialize, Default)]
struct ListingData {
    #[serde(default)]
    children: Vec<Child>,
}

#[derive(Debug, Deserialize)]
struct Child {
    #[serde(default)]
    data: PostData,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct PostData {
    #[serde(default)]
    title: String,
    #[serde(default)]
    selftext: String,
}

pub struct RedditWsbProvider {
    http: Arc<dyn HttpFetcher>,
    url: String,
    filter: TickerFilterConfig,
}

impl RedditWsbProvider {
    pub fn new(http: Arc<dyn HttpFetcher>) -> Self {
        Self {
            http,
            url: REDDIT_DEFAULT_URL.to_string(),
            filter: TickerFilterConfig::default(),
        }
    }

    #[allow(dead_code)] // builder used by unit tests + future settings overrides
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    #[allow(dead_code)] // public seam for callers with curated symbol whitelists
    pub fn with_filter(mut self, filter: TickerFilterConfig) -> Self {
        self.filter = filter;
        self
    }

    fn count_mentions(&self, posts: &[PostData], symbols: &[String]) -> Vec<(String, i64)> {
        // Pre-build a per-symbol whitelist filter so we can credit bare
        // uppercase tokens (Reddit titles often use "TSLA pumping" with
        // no `$`).
        let wanted: std::collections::HashSet<String> =
            symbols.iter().map(|s| s.to_ascii_uppercase()).collect();
        let scoped = TickerFilterConfig {
            whitelist: Some(wanted.clone()),
            ..self.filter.clone()
        };

        let mut counts: std::collections::HashMap<String, i64> =
            wanted.iter().map(|s| (s.clone(), 0_i64)).collect();
        for post in posts {
            let combined = format!("{}\n{}", post.title, post.selftext);
            for sym in extract_valid_tickers(&combined, &scoped) {
                if let Some(c) = counts.get_mut(&sym) {
                    *c += 1;
                }
            }
        }
        let mut out: Vec<(String, i64)> = counts.into_iter().collect();
        out.sort_by_key(|(sym, _)| sym.clone());
        out
    }
}

#[async_trait]
impl SentimentProvider for RedditWsbProvider {
    fn id(&self) -> &'static str {
        SentimentSource::RedditWsb.as_str()
    }

    async fn fetch(&self, symbols: &[String]) -> Result<Vec<SentimentSample>, String> {
        if symbols.is_empty() {
            return Ok(Vec::new());
        }
        let body = self
            .http
            .get_text(&self.url, &[("Accept", "application/json")])
            .await?;
        let listing: RedditListing =
            serde_json::from_str(&body).map_err(|e| format!("reddit parse: {e}"))?;
        let posts: Vec<PostData> = listing.data.children.into_iter().map(|c| c.data).collect();

        let counts = self.count_mentions(&posts, symbols);
        let mut out = Vec::with_capacity(counts.len());
        for (symbol, mentions) in counts {
            let raw_payload = serde_json::to_string(&serde_json::json!({
                "subreddit": "wallstreetbets",
                "posts_scanned": posts.len(),
                "mentions_in_listing": mentions,
            }))
            .map_err(|e| format!("reddit raw payload: {e}"))?;
            out.push(SentimentSample {
                source: SentimentSource::RedditWsb,
                symbol,
                // Reddit free-text scrape is mention-count-only — we
                // don't fake a polarity score from titles.
                score: None,
                mentions_24h: Some(mentions),
                label: None,
                rank: None,
                raw_payload,
                is_stale: mentions == 0,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::social_sentiment::provider::MockHttpFetcher;

    #[tokio::test]
    async fn counts_cashtag_and_bare_mentions_for_requested_symbols() {
        let http = Arc::new(MockHttpFetcher::new());
        let body = r#"{
            "data": {"children": [
                {"data": {"title": "$TSLA to the moon", "selftext": "TSLA YOLO"}},
                {"data": {"title": "AMD vs NVDA earnings preview", "selftext": ""}},
                {"data": {"title": "Random JPOW take", "selftext": "$AAPL is fine but $A is just a letter"}}
            ]}
        }"#;
        http.respond_with("https://test.local/wsb.json", body);
        let provider = RedditWsbProvider::new(http).with_url("https://test.local/wsb.json");
        let out = provider
            .fetch(&["TSLA".into(), "AMD".into(), "AAPL".into()])
            .await
            .expect("ok");

        let by_sym: std::collections::HashMap<_, _> = out
            .iter()
            .map(|s| (s.symbol.clone(), s.mentions_24h.unwrap_or(0)))
            .collect();
        // One mention per post the ticker appears in (dedup within a
        // post: "$TSLA … TSLA" still counts as one post-level mention).
        assert_eq!(by_sym.get("TSLA").copied(), Some(1), "post 1 mentions TSLA");
        assert_eq!(by_sym.get("AMD").copied(), Some(1), "post 2 mentions AMD");
        assert_eq!(by_sym.get("AAPL").copied(), Some(1), "post 3 mentions AAPL");
        // The blocklist filter (`$A`) prevents the bogus "$A is just a letter" hit.
    }

    #[tokio::test]
    async fn no_mentions_yields_stale_row() {
        let http = Arc::new(MockHttpFetcher::new());
        http.respond_with(
            "https://test.local/wsb.json",
            r#"{"data": {"children": []}}"#,
        );
        let provider = RedditWsbProvider::new(http).with_url("https://test.local/wsb.json");
        let out = provider.fetch(&["TSLA".into()]).await.expect("ok");
        assert_eq!(out.len(), 1);
        assert!(out[0].is_stale);
        assert_eq!(out[0].mentions_24h, Some(0));
    }
}
