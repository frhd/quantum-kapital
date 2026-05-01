//! `get_sentiment` — durable social-sentiment snapshot per symbol.
//!
//! Read-only over `social_sentiment` (V04). The agent / interactive
//! caller asks for a window (`since`, default 24h) and optionally a
//! source subset; the tool replies with one [`SourceSummary`] per
//! source — `latest` row + the in-window samples.
//!
//! The tool does NOT trigger a refresh. Sentiment is filled by the
//! [`SocialSentimentScheduler`] on its own cadence; if `latest` is
//! older than expected the agent should treat that as a coverage gap
//! rather than a tool error.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, serde_json::json, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;

/// Default window lookback when the caller omits `since_unix`. 24h
/// matches the "recent WSB mention count" exit criterion in the
/// phase plan.
const DEFAULT_LOOKBACK_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Deserialize, JsonSchema, Default)]
pub struct GetSentimentArgs {
    /// Ticker symbol (case-insensitive).
    pub symbol: String,
    /// Inclusive lower bound on `fetched_at`, unix seconds. When
    /// omitted the tool defaults to "last 24 hours".
    #[serde(default)]
    pub since_unix: Option<i64>,
    /// Restrict to a subset of sources. Valid values:
    /// `"reddit_wsb"`, `"stocktwits"`, `"apewisdom"`. Omit / pass an
    /// empty array for all sources.
    #[serde(default)]
    pub sources: Option<Vec<String>>,
}

#[tool_router(router = get_sentiment_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_sentiment",
        description = "Return durable social-sentiment for `symbol` over a trailing window (default 24h). Replies with one summary per source (`reddit_wsb`, `stocktwits`, `apewisdom`): the freshest sample + every in-window sample. Read-only — sentiment is refreshed by the in-app scheduler on its own cadence; use `latest.fetched_at_unix` to assess freshness. `score` is a normalised polarity in [-1, 1] when the source publishes one; raw mention counts are in `mentions_24h`."
    )]
    pub async fn get_sentiment(
        &self,
        Parameters(args): Parameters<GetSentimentArgs>,
    ) -> Result<CallToolResult, McpError> {
        let symbol = args.symbol.trim().to_uppercase();
        if symbol.is_empty() {
            return map_tool_result::<(), &str>(Err("symbol must not be empty"));
        }
        let now = chrono::Utc::now().timestamp();
        let since = args
            .since_unix
            .unwrap_or_else(|| now - DEFAULT_LOOKBACK_SECS);
        let sources = args
            .sources
            .filter(|v| !v.is_empty())
            .map(|v| v.into_iter().collect::<Vec<_>>());

        let summaries = self
            .social_sentiment
            .snapshot(&symbol, since, sources)
            .await
            .map_err(|e| McpError::internal_error(format!("snapshot: {e}"), None))?;

        Ok(CallToolResult::structured(json!({
            "symbol": symbol,
            "since_unix": since,
            "now_unix": now,
            "by_source": summaries,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use crate::services::social_sentiment::repo::insert_sample;
    use crate::services::social_sentiment::types::{
        SentimentLabel, SentimentSample, SentimentSource,
    };
    use std::sync::Arc;

    fn sample(
        source: SentimentSource,
        symbol: &str,
        score: Option<f64>,
        mentions: Option<i64>,
        label: Option<SentimentLabel>,
    ) -> SentimentSample {
        SentimentSample {
            source,
            symbol: symbol.into(),
            score,
            mentions_24h: mentions,
            label,
            rank: None,
            raw_payload: "{}".into(),
            is_stale: false,
        }
    }

    #[tokio::test]
    async fn get_sentiment_returns_snapshot_with_three_sources() {
        let (_tmp, db) = make_db();
        let now = chrono::Utc::now().timestamp();
        // Seed one row per source within the trailing 24h window.
        for s in [
            sample(SentimentSource::Apewisdom, "TSLA", Some(0.6), Some(420), Some(SentimentLabel::Bullish)),
            sample(SentimentSource::Stocktwits, "TSLA", Some(0.2), Some(31), Some(SentimentLabel::Bullish)),
            sample(SentimentSource::RedditWsb, "TSLA", None, Some(15), None),
        ] {
            insert_sample(Arc::clone(&db), s, now - 30).await.unwrap();
        }

        let handler = handler_for_db(db);
        let result = handler
            .get_sentiment(Parameters(GetSentimentArgs {
                symbol: "tsla".into(),
                since_unix: None,
                sources: None,
            }))
            .await
            .expect("ok");
        assert_eq!(result.is_error, Some(false));
        let body = result.structured_content.expect("structured");
        assert_eq!(body["symbol"].as_str().unwrap(), "TSLA");
        let by_source = body["by_source"].as_array().expect("by_source");
        let source_ids: Vec<&str> = by_source
            .iter()
            .map(|s| s["source"].as_str().unwrap())
            .collect();
        for sid in ["apewisdom", "stocktwits", "reddit_wsb"] {
            assert!(source_ids.contains(&sid), "missing source {sid}");
        }
        let ape = by_source
            .iter()
            .find(|s| s["source"] == "apewisdom")
            .unwrap();
        assert_eq!(ape["latest"]["score"].as_f64().unwrap(), 0.6);
        assert_eq!(ape["latest"]["mentions_24h"].as_i64().unwrap(), 420);
    }

    #[tokio::test]
    async fn get_sentiment_filters_by_source_subset() {
        let (_tmp, db) = make_db();
        let now = chrono::Utc::now().timestamp();
        for s in [
            sample(SentimentSource::Apewisdom, "TSLA", Some(0.5), None, None),
            sample(SentimentSource::Stocktwits, "TSLA", Some(0.3), None, None),
        ] {
            insert_sample(Arc::clone(&db), s, now - 30).await.unwrap();
        }
        let handler = handler_for_db(db);
        let result = handler
            .get_sentiment(Parameters(GetSentimentArgs {
                symbol: "TSLA".into(),
                since_unix: None,
                sources: Some(vec!["stocktwits".into()]),
            }))
            .await
            .expect("ok");
        let body = result.structured_content.unwrap();
        let by_source = body["by_source"].as_array().unwrap();
        assert_eq!(by_source.len(), 1);
        assert_eq!(by_source[0]["source"].as_str().unwrap(), "stocktwits");
    }

    #[tokio::test]
    async fn get_sentiment_empty_symbol_is_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let result = handler
            .get_sentiment(Parameters(GetSentimentArgs {
                symbol: "   ".into(),
                ..Default::default()
            }))
            .await
            .expect("ok");
        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn get_sentiment_no_data_returns_empty_by_source_array() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let result = handler
            .get_sentiment(Parameters(GetSentimentArgs {
                symbol: "ZZZZ".into(),
                ..Default::default()
            }))
            .await
            .expect("ok");
        assert_eq!(result.is_error, Some(false));
        let body = result.structured_content.unwrap();
        assert!(body["by_source"].as_array().unwrap().is_empty());
    }
}
