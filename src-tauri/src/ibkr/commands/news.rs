//! Workspace Phase 3 — read-only Tauri command over the `news_cache`
//! SQLite table. Producer-agnostic: any provider that calls
//! [`services::news_cache::write_cache`] (today: `IbkrNewsProvider`)
//! makes rows visible here, including the optional LLM verdict that the
//! `news_interpreter` populates after the fact.
//!
//! Distinct from `tracker_get_news`, which goes through `NewsProvider`
//! and may attempt an upstream refresh. This command never triggers a
//! fetch — the workspace News panel renders whatever the cache holds
//! and surfaces `fetched_at_unix` so the user can judge freshness.

use std::sync::Arc;
use tauri::State;

use crate::ibkr::types::news::NewsItem;
use crate::services::news_cache::read_cache_with_verdict;
use crate::storage::Db;

/// Read-only view returned by [`news_get_cached`]. Mirrors the columns
/// `services::news_cache::CachedNews` exposes, with the verdict
/// column passed through as raw JSON so the UI can decode whatever
/// schema the interpreter currently writes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CachedTickerNews {
    pub symbol: String,
    pub items: Vec<NewsItem>,
    pub verdict_json: Option<String>,
    pub fetched_at_unix: i64,
}

#[tauri::command]
pub async fn news_get_cached(
    db: State<'_, Arc<Db>>,
    symbol: String,
) -> Result<CachedTickerNews, String> {
    let symbol = symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return Err("symbol must not be empty".to_string());
    }

    let cached = read_cache_with_verdict(&db, &symbol)
        .await
        .map_err(|e| format!("read_cache_with_verdict: {e}"))?;

    Ok(match cached {
        Some(c) => CachedTickerNews {
            symbol,
            items: c.items,
            verdict_json: c.verdict_json,
            fetched_at_unix: c.fetched_at,
        },
        None => CachedTickerNews {
            symbol,
            items: Vec::new(),
            verdict_json: None,
            fetched_at_unix: 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::news_cache::{write_cache, write_verdict};
    use crate::storage::Db;
    use chrono::Utc;
    use tempfile::NamedTempFile;

    fn open_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    fn sample_item() -> NewsItem {
        NewsItem {
            time_published: Utc::now(),
            title: "Apple beats earnings".to_string(),
            summary: "Q4 numbers above consensus.".to_string(),
            source: "Reuters".to_string(),
            url: "https://example.com/aapl-q4".to_string(),
            overall_sentiment_score: Some(0.45),
            overall_sentiment_label: Some("Bullish".to_string()),
            ticker_sentiment: vec![],
        }
    }

    /// Mirrors the body of `news_get_cached` so the bulk of the logic
    /// is testable without Tauri `State` extraction.
    async fn call(db: Arc<Db>, symbol: &str) -> Result<CachedTickerNews, String> {
        let symbol = symbol.trim().to_uppercase();
        if symbol.is_empty() {
            return Err("symbol must not be empty".to_string());
        }
        let cached = read_cache_with_verdict(&db, &symbol)
            .await
            .map_err(|e| format!("read_cache_with_verdict: {e}"))?;
        Ok(match cached {
            Some(c) => CachedTickerNews {
                symbol,
                items: c.items,
                verdict_json: c.verdict_json,
                fetched_at_unix: c.fetched_at,
            },
            None => CachedTickerNews {
                symbol,
                items: Vec::new(),
                verdict_json: None,
                fetched_at_unix: 0,
            },
        })
    }

    /// Cache hit path — we should see the items, the verdict JSON, and
    /// the persisted `fetched_at_unix`.
    #[tokio::test]
    async fn returns_cached_payload_with_verdict() {
        let (_tmp, db) = open_db();
        let now = Utc::now().timestamp();
        let item = sample_item();
        write_cache(&db, "AAPL", now, std::slice::from_ref(&item))
            .await
            .unwrap();
        write_verdict(
            &db,
            "AAPL",
            r#"{"tone":"bullish","ep_worthy":true,"parabolic_risk":false,"summary":"earnings beat"}"#,
        )
        .await
        .unwrap();

        let view = call(db, "aapl").await.expect("ok");
        assert_eq!(view.symbol, "AAPL");
        assert_eq!(view.items.len(), 1);
        assert_eq!(view.items[0].title, "Apple beats earnings");
        assert_eq!(view.fetched_at_unix, now);
        let verdict = view.verdict_json.expect("verdict json present");
        assert!(verdict.contains("bullish"));
    }

    /// Cache miss path — an empty list, `fetched_at_unix == 0`, no
    /// verdict. Lets the panel distinguish "no news yet" from "no row".
    #[tokio::test]
    async fn returns_empty_when_symbol_uncached() {
        let (_tmp, db) = open_db();
        let view = call(db, "TSLA").await.expect("ok");
        assert_eq!(view.symbol, "TSLA");
        assert!(view.items.is_empty());
        assert!(view.verdict_json.is_none());
        assert_eq!(view.fetched_at_unix, 0);
    }

    #[tokio::test]
    async fn rejects_empty_symbol() {
        let (_tmp, db) = open_db();
        let err = call(db, "   ")
            .await
            .expect_err("empty symbol should error");
        assert!(err.contains("symbol"), "got: {err}");
    }
}
