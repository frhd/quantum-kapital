//! `get_watchlist_briefing` — single MCP call returning quote + bars +
//! news + sentiment + setups + fundamentals for every (or a filtered
//! subset of) watchlist symbol. Per-symbol error envelope; concurrent
//! fan-out. Replaces the 12+ tool-call fan-out previously needed to
//! brief the watchlist before producing a morning playbook.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::bars::GetBarsArgs;
use crate::mcp::tools::fundamentals::GetFundamentalsArgs;
use crate::mcp::tools::get_sentiment::GetSentimentArgs;
use crate::mcp::tools::map_tool_result;
use crate::mcp::tools::news::GetNewsArgs;
use crate::mcp::tools::quote::GetQuoteArgs;
use crate::mcp::tools::setups::GetSetupsArgs;
use crate::services::watchlist_briefing::{
    compose, BarsFetcher, BriefingFetchers, BriefingOpts, FetchResult, Future01, SymbolFetcher,
};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetWatchlistBriefingArgs {
    /// Optional symbol allow-list. Omit to brief every watchlist row.
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// Daily-bars lookback. Defaults to 15.
    #[serde(default)]
    pub lookback_days: Option<u32>,
    /// Bar size; defaults to "1d". One of "1m", "5m", "15m", "1h", "1d".
    #[serde(default)]
    pub bar_size: Option<String>,
    /// News cache freshness window (seconds). Defaults to 3600.
    #[serde(default)]
    pub news_max_age_secs: Option<u32>,
}

#[tool_router(router = get_watchlist_briefing_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_watchlist_briefing",
        description = "One-shot per-symbol briefing for the watchlist (or a `symbols` subset). Returns `{ as_of, symbols: [...], items: [{ symbol, quote, bars, news, sentiment, setups, fundamentals, errors }, ...] }` with each constituent fetched concurrently. Per-symbol `errors[]` lists any partial failures (e.g. `\"news: upstream_failed\"`) — successful fields remain populated. Defaults: `lookback_days=15`, `bar_size=\"1d\"`, `news_max_age_secs=3600`. Replaces the 12+ tool-call fan-out previously needed to brief the watchlist."
    )]
    pub async fn get_watchlist_briefing(
        &self,
        Parameters(args): Parameters<GetWatchlistBriefingArgs>,
    ) -> Result<CallToolResult, McpError> {
        let symbols: Vec<String> = match args.symbols {
            Some(s) => s
                .into_iter()
                .map(|x| x.trim().to_uppercase())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        let symbols = if symbols.is_empty() {
            match self.read_watchlist_symbols().await {
                Ok(v) => v,
                Err(e) => return map_tool_result::<(), String>(Err(e)),
            }
        } else {
            symbols
        };

        let opts = BriefingOpts {
            lookback_days: args.lookback_days.unwrap_or(15),
            bars_size: args.bar_size.unwrap_or_else(|| "1d".into()),
            news_max_age_secs: args.news_max_age_secs.unwrap_or(3600),
            concurrency: 4,
        };

        if symbols.is_empty() {
            return map_tool_result::<_, String>(Ok(serde_json::json!({
                "as_of": chrono::Utc::now().timestamp(),
                "symbols": Vec::<String>::new(),
                "items": Vec::<Value>::new(),
            })));
        }

        let fetchers = build_fetchers(self, opts.news_max_age_secs);
        let out = compose(symbols, opts, &fetchers).await;
        map_tool_result::<_, String>(Ok(
            serde_json::to_value(out).expect("WatchlistBriefing serializes")
        ))
    }

    async fn read_watchlist_symbols(&self) -> Result<Vec<String>, String> {
        let rows = self.tracker.list(None).await.map_err(|e| e.to_string())?;
        let mut symbols: Vec<String> = rows.into_iter().map(|r| r.symbol.to_uppercase()).collect();
        symbols.sort();
        symbols.dedup();
        Ok(symbols)
    }
}

fn build_fetchers(handler: &McpHandler, news_max_age_secs: u32) -> BriefingFetchers {
    BriefingFetchers {
        fetch_quote: prod_fetch_quote(handler),
        fetch_bars: prod_fetch_bars(handler),
        fetch_news: prod_fetch_news(handler, news_max_age_secs),
        fetch_sentiment: prod_fetch_sentiment(handler),
        fetch_setups: prod_fetch_setups(handler),
        fetch_fundamentals: prod_fetch_fundamentals(handler),
    }
}

fn extract_value(r: CallToolResult) -> FetchResult {
    if r.is_error == Some(true) {
        let msg = r
            .content
            .iter()
            .find_map(|c| c.as_text().map(|t| t.text.clone()))
            .unwrap_or_else(|| "tool error".to_string());
        return Err(msg);
    }
    Ok(r.structured_content.unwrap_or(Value::Null))
}

fn prod_fetch_quote(handler: &McpHandler) -> SymbolFetcher {
    let h = handler.clone();
    Box::new(move |sym: &str| -> Future01<'static> {
        let h = h.clone();
        let s = sym.to_string();
        Box::pin(async move {
            let r = h
                .get_quote(Parameters(GetQuoteArgs { symbol: s }))
                .await
                .map_err(|e| e.to_string())?;
            extract_value(r)
        })
    })
}

fn prod_fetch_bars(handler: &McpHandler) -> BarsFetcher {
    let h = handler.clone();
    Box::new(
        move |sym: &str, size: &str, lookback: u32| -> Future01<'static> {
            let h = h.clone();
            let s = sym.to_string();
            let bs = size.to_string();
            Box::pin(async move {
                let r = h
                    .get_bars(Parameters(GetBarsArgs {
                        symbol: s,
                        bar_size: bs,
                        lookback_days: lookback,
                    }))
                    .await
                    .map_err(|e| e.to_string())?;
                extract_value(r)
            })
        },
    )
}

fn prod_fetch_news(handler: &McpHandler, max_age_secs: u32) -> SymbolFetcher {
    let h = handler.clone();
    Box::new(move |sym: &str| -> Future01<'static> {
        let h = h.clone();
        let s = sym.to_string();
        Box::pin(async move {
            let r = h
                .get_news(Parameters(GetNewsArgs {
                    symbol: s,
                    max_age_secs: Some(max_age_secs),
                }))
                .await
                .map_err(|e| e.to_string())?;
            extract_value(r)
        })
    })
}

fn prod_fetch_sentiment(handler: &McpHandler) -> SymbolFetcher {
    let h = handler.clone();
    Box::new(move |sym: &str| -> Future01<'static> {
        let h = h.clone();
        let s = sym.to_string();
        Box::pin(async move {
            let r = h
                .get_sentiment(Parameters(GetSentimentArgs {
                    symbol: s,
                    since_unix: None,
                    sources: None,
                }))
                .await
                .map_err(|e| e.to_string())?;
            extract_value(r)
        })
    })
}

fn prod_fetch_setups(handler: &McpHandler) -> SymbolFetcher {
    let h = handler.clone();
    Box::new(move |sym: &str| -> Future01<'static> {
        let h = h.clone();
        let s = sym.to_string();
        Box::pin(async move {
            let r = h
                .get_setups(Parameters(GetSetupsArgs {
                    symbol: Some(s),
                    since: None,
                }))
                .await
                .map_err(|e| e.to_string())?;
            extract_value(r)
        })
    })
}

fn prod_fetch_fundamentals(handler: &McpHandler) -> SymbolFetcher {
    let h = handler.clone();
    Box::new(move |sym: &str| -> Future01<'static> {
        let h = h.clone();
        let s = sym.to_string();
        Box::pin(async move {
            let r = h
                .get_fundamentals(Parameters(GetFundamentalsArgs { symbol: s }))
                .await
                .map_err(|e| e.to_string())?;
            extract_value(r)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::ibkr::types::tracker::{StrategyTag, TrackerSource};
    use crate::ibkr::types::BarSize;
    use crate::mcp::tools::test_support::{handler_for_mock_ibkr, make_db};
    use crate::storage::Db;
    use chrono::TimeZone;
    use std::sync::Arc;

    /// Seed today's daily bar for `symbol` so `get_bars(lookback_days=1)`
    /// is served from cache and the [`PanickingFetcher`] is never invoked.
    async fn seed_today_daily_bar(db: &Arc<Db>, symbol: &str, close: f64) {
        let today = chrono::Utc::now().date_naive();
        let bar_time = chrono::Utc
            .from_utc_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
            .timestamp();
        let bar_size_str = BarSize::Day1.as_str().to_string();
        let symbol = symbol.to_string();
        db.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO bars_cache \
                 (symbol, bar_size, bar_time, open, high, low, close, volume, wap) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    symbol,
                    bar_size_str,
                    bar_time,
                    close - 0.5,
                    close + 1.0,
                    close - 1.0,
                    close,
                    1_000_i64,
                    close,
                ],
            )?;
            Ok(())
        })
        .await
        .expect("seed bars_cache");
    }

    /// A briefing across two seeded watchlist tickers returns one item per
    /// symbol. Each item carries `quote` populated from the
    /// `MockIbkrClient` snapshot; `bars` from the seeded cache; `news` /
    /// `sentiment` / `setups` return their empty-but-successful shapes;
    /// `fundamentals` fake errors with `NotFound`, surfaced via `errors`.
    #[tokio::test]
    async fn returns_one_item_per_watchlist_symbol() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(Arc::clone(&db), mock).await;
        for sym in ["AMD", "TSLA"] {
            handler
                .tracker
                .add(
                    sym,
                    TrackerSource::Manual,
                    None,
                    vec![StrategyTag::Breakout],
                    None,
                )
                .await
                .unwrap();
            seed_today_daily_bar(&db, sym, 100.0).await;
        }

        let result = handler
            .get_watchlist_briefing(Parameters(GetWatchlistBriefingArgs {
                lookback_days: Some(1),
                ..Default::default()
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result.structured_content.expect("structured");
        let items = body["items"].as_array().expect("items array");
        assert_eq!(items.len(), 2);
        let symbols: Vec<&str> = items
            .iter()
            .map(|i| i["symbol"].as_str().unwrap())
            .collect();
        assert_eq!(symbols, vec!["AMD", "TSLA"]);
        let amd = &items[0];
        assert!(amd["quote"]["lastPrice"].is_number(), "amd quote: {amd}");
        assert_eq!(amd["bars"]["count"].as_u64().unwrap(), 1);
        assert!(amd["news"].is_object(), "news envelope: {amd}");
        assert!(amd["sentiment"].is_object(), "sentiment envelope: {amd}");
        assert!(amd["setups"].is_object(), "setups envelope: {amd}");
        // Fundamentals fake returns NotFound → fundamentals key omitted,
        // entry surfaces in `errors`.
        assert!(amd.get("fundamentals").is_none());
        let errors = amd["errors"].as_array().expect("errors array");
        assert!(
            errors
                .iter()
                .any(|e| e.as_str().is_some_and(|s| s.starts_with("fundamentals:"))),
            "errors: {errors:?}",
        );
    }

    #[tokio::test]
    async fn explicit_symbols_arg_overrides_watchlist() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(Arc::clone(&db), mock).await;
        // Watchlist has TSLA, but caller asks for AMD.
        handler
            .tracker
            .add("TSLA", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        seed_today_daily_bar(&db, "AMD", 200.0).await;
        let result = handler
            .get_watchlist_briefing(Parameters(GetWatchlistBriefingArgs {
                symbols: Some(vec!["amd".into()]),
                lookback_days: Some(1),
                ..Default::default()
            }))
            .await
            .expect("rmcp Ok");
        let body = result.structured_content.expect("structured");
        let items = body["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["symbol"].as_str().unwrap(), "AMD");
    }

    #[tokio::test]
    async fn empty_watchlist_returns_empty_items_envelope() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;
        let result = handler
            .get_watchlist_briefing(Parameters(GetWatchlistBriefingArgs::default()))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false));
        let body = result.structured_content.expect("structured");
        assert!(body["items"].as_array().unwrap().is_empty());
    }

    /// Read-only audit invariant — the tool must NOT write `mcp_audit` rows.
    #[tokio::test]
    async fn get_watchlist_briefing_does_not_write_audit() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(Arc::clone(&db), mock).await;
        handler
            .tracker
            .add("TSLA", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        seed_today_daily_bar(&db, "TSLA", 250.0).await;

        let _ = handler
            .get_watchlist_briefing(Parameters(GetWatchlistBriefingArgs {
                lookback_days: Some(1),
                ..Default::default()
            }))
            .await
            .expect("rmcp Ok");

        let audits = crate::services::mcp_audit::list(&db, 100, 0)
            .await
            .expect("list");
        assert!(audits.is_empty(), "got {:?}", audits);
    }
}
