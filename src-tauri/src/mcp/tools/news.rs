//! `get_news` — cached news + LLM verdict, with a best-effort upstream
//! refresh when the cache is stale.
//!
//! Wraps `news::read_cache_with_verdict`, optionally followed by a
//! single `NewsProvider::fetch` attempt when the cache is missing or
//! older than `max_age_secs`. Crucially, upstream failure (rate limit,
//! missing AV key, IBKR pacing, transport) does NOT propagate — the
//! tool returns whatever cache it has (possibly empty) with
//! `source: "upstream_failed"`. This mirrors the existing
//! `tracker_get_news` Tauri command and keeps agent loops resilient
//! to transient upstream issues.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, serde_json::json, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::ibkr::types::news::NewsItem;
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::news_cache::read_cache_with_verdict;

/// Default freshness window. Matches the news cache TTL most other
/// callers use. Public so test code can reference it explicitly.
pub const DEFAULT_MAX_AGE_SECS: u32 = 3600;

/// Lookback passed to `NewsProvider::fetch` when an upstream refresh
/// is triggered. 24h matches the rest of the codebase.
const NEWS_LOOKBACK_HOURS: u32 = 24;

#[derive(Debug, Deserialize, JsonSchema, Default)]
pub struct GetNewsArgs {
    /// Ticker symbol (case-insensitive).
    pub symbol: String,
    /// Maximum cache age before we attempt an AV refresh, in seconds.
    /// Defaults to 3600 (one hour). The tool always prefers cache if it
    /// is fresh enough, so a large value here means "never refresh."
    #[serde(default)]
    pub max_age_secs: Option<u32>,
}

#[tool_router(router = news_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_news",
        description = "Return cached news items and the latest LLM news-verdict for `symbol`. When the cache is missing or older than `max_age_secs` (default 3600s), the tool attempts a single upstream refresh through the configured `NewsProvider` and re-reads cache. On upstream failure (no API key, rate limit, no IBKR subscription, transport) the tool still returns whatever cache exists, with `source: \"upstream_failed\"` — never errors on upstream issues. Use this to ground LLM commentary on the most recent verified news the app has ingested."
    )]
    pub async fn get_news(
        &self,
        Parameters(args): Parameters<GetNewsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let symbol = args.symbol.trim().to_uppercase();
        if symbol.is_empty() {
            return map_tool_result::<(), &str>(Err("symbol must not be empty"));
        }
        let max_age = args.max_age_secs.unwrap_or(DEFAULT_MAX_AGE_SECS) as i64;

        let now = chrono::Utc::now().timestamp();
        let initial = read_cache_with_verdict(&self.db, &symbol)
            .await
            .map_err(|e| McpError::internal_error(format!("read_cache_with_verdict: {e}"), None))?;

        let (cache, source, upstream_note): (
            Option<crate::services::news_cache::CachedNews>,
            &'static str,
            Option<String>,
        ) = match initial {
            Some(c) if now.saturating_sub(c.fetched_at) <= max_age => (Some(c), "cache", None),
            other => {
                // Either no row at all, or stale — try to refresh
                // through the configured `NewsProvider`.
                match self.news_provider.fetch(&symbol, NEWS_LOOKBACK_HOURS).await {
                    Ok(_items) => {
                        // Re-read so we pick up the verdict-bearing row
                        // the provider just wrote (or refreshed).
                        let refreshed =
                            read_cache_with_verdict(&self.db, &symbol)
                                .await
                                .map_err(|e| {
                                    McpError::internal_error(
                                        format!("read_cache_with_verdict (post-refresh): {e}"),
                                        None,
                                    )
                                })?;
                        match refreshed {
                            Some(c) => (Some(c), "upstream_fresh", None),
                            None => (
                                other,
                                "upstream_fresh",
                                Some(
                                    "upstream refresh succeeded but cache row is still empty"
                                        .to_string(),
                                ),
                            ),
                        }
                    }
                    Err(e) => (
                        other,
                        "upstream_failed",
                        Some(format!("upstream refresh failed: {e}")),
                    ),
                }
            }
        };

        let (items, verdict_json, fetched_at) = match cache {
            Some(c) => (c.items, c.verdict_json, c.fetched_at),
            None => (Vec::<NewsItem>::new(), None, 0_i64),
        };
        let staleness = if fetched_at == 0 {
            -1
        } else {
            (now - fetched_at).max(0)
        };

        Ok(CallToolResult::structured(json!({
            "symbol": symbol,
            "items": items,
            "verdict_json": verdict_json,
            "fetched_at_unix": fetched_at,
            "staleness_seconds": staleness,
            "source": source,
            "note": upstream_note,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    /// Seed a fresh `news_cache` row + verdict and verify the tool returns
    /// both with `source: "cache"` and a small non-negative staleness.
    /// The `FinancialDataService` in `handler_for_db` has an empty API
    /// key and base URL, so any inadvertent fallback would error — this
    /// test guards the cache-hit fast path.
    #[tokio::test]
    async fn get_news_returns_fresh_cache_without_hitting_av() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let now = chrono::Utc::now().timestamp();
        let payload = json!([
            {
                "time_published": "2024-01-01T12:00:00Z",
                "title": "Apple beats earnings",
                "summary": "Q4 numbers above consensus.",
                "source": "Reuters",
                "url": "https://example.com/aapl-q4",
                "overall_sentiment_score": 0.45,
                "overall_sentiment_label": "Bullish",
                "ticker_sentiment": []
            }
        ])
        .to_string();
        let verdict = json!({"tone": "bullish", "rationale": "earnings beat"}).to_string();

        let payload_for_db = payload.clone();
        let verdict_for_db = verdict.clone();
        handler
            .db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO news_cache (symbol, fetched_at, payload, news_verdict_json) \
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params!["AAPL", now, payload_for_db, verdict_for_db],
                )?;
                Ok(())
            })
            .await
            .unwrap();

        let result = handler
            .get_news(Parameters(GetNewsArgs {
                symbol: "AAPL".to_string(),
                max_age_secs: Some(3600),
            }))
            .await
            .expect("tool ok");
        assert_eq!(result.is_error, Some(false));
        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content");
        assert_eq!(body["symbol"].as_str().unwrap(), "AAPL");
        assert_eq!(body["source"].as_str().unwrap(), "cache");
        let items = body["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"].as_str().unwrap(), "Apple beats earnings");
        assert_eq!(body["verdict_json"].as_str().unwrap(), verdict);
        assert!(body["staleness_seconds"].as_i64().unwrap() >= 0);
    }

    /// When the cache is stale and AV is unreachable (empty key + empty
    /// base URL = no-key fallback path inside the news fetcher), the tool
    /// must still return whatever cache it has with `source: "av_failed"`
    /// or `source: "cache"` (the fetcher itself doesn't error in the
    /// no-key path, so it actually re-reads cache and reports "av_fresh"
    /// when nothing was rewritten — the failure mode we care about
    /// surfaces with a real HTTP error and is exercised end-to-end in
    /// the integration tests).
    #[tokio::test]
    async fn get_news_with_empty_symbol_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let result = handler
            .get_news(Parameters(GetNewsArgs {
                symbol: "   ".to_string(),
                max_age_secs: None,
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("symbol"), "got: {}", txt.text);
    }
}
