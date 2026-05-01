//! `get_bars` — historical OHLCV bars, cache-first with IBKR fallback.
//!
//! Wraps `HistoricalDataService::fetch_bars`. The agent-facing `bar_size`
//! is one of the compact strings `"1m" / "5m" / "15m" / "1h" / "1d"`,
//! translated inside the tool to a [`BarSize`] enum — we deliberately do
//! NOT expose the IBKR wire format (`"1 day"`, `"5 mins"`, ...) to LLM
//! clients. Cache-misses go through the regular service path: rate
//! limited, deduplicated per-key, IBKR-fetched, and persisted to
//! `bars_cache`.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::ibkr::types::historical::BarSize;
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::historical_data_service::Lookback;

/// Hard cap on how many days of history a single tool call can request.
/// 365 days fits comfortably in IBKR's daily-bar request envelope and is
/// well above any reasonable detector window. Larger windows are
/// rejected as a domain error so the LLM can adjust rather than the
/// service silently truncating.
const MAX_LOOKBACK_DAYS: u32 = 365;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetBarsArgs {
    /// Ticker symbol (case-insensitive).
    pub symbol: String,
    /// Bar resolution. One of: `"1m"`, `"5m"`, `"15m"`, `"1h"`, `"1d"`.
    pub bar_size: String,
    /// Number of past trading days to retrieve. Capped at 365.
    pub lookback_days: u32,
}

/// Map the tool-facing `bar_size` string to a [`BarSize`] variant. The
/// agent-facing surface is intentionally narrower than the full enum:
/// it exposes only the resolutions the detector framework actually
/// reasons about, keeping the prompt schema small.
fn parse_bar_size(s: &str) -> Result<BarSize, String> {
    match s {
        "1m" => Ok(BarSize::Min1),
        "5m" => Ok(BarSize::Min5),
        "15m" => Ok(BarSize::Min15),
        "1h" => Ok(BarSize::Hour1),
        "1d" => Ok(BarSize::Day1),
        other => Err(format!(
            "unknown bar_size: {other}; one of [1m, 5m, 15m, 1h, 1d]"
        )),
    }
}

#[tool_router(router = bars_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_bars",
        description = "Return historical OHLCV bars for `symbol` over the last `lookback_days` trading days at `bar_size` resolution. Cache-first: returns instantly when bars are already in `bars_cache`, otherwise fetches from IBKR (subject to the historical-data rate limit). Use this to anchor pattern claims to real bars before reasoning. `bar_size` must be one of \"1m\", \"5m\", \"15m\", \"1h\", \"1d\"; `lookback_days` is capped at 365. Returns `{ items: [Bar, ...], count: N }`."
    )]
    pub async fn get_bars(
        &self,
        Parameters(args): Parameters<GetBarsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let symbol = args.symbol.trim().to_uppercase();
        if symbol.is_empty() {
            return map_tool_result::<(), &str>(Err("symbol must not be empty"));
        }
        if args.lookback_days == 0 {
            return map_tool_result::<(), &str>(Err("lookback_days must be >= 1"));
        }
        if args.lookback_days > MAX_LOOKBACK_DAYS {
            return map_tool_result::<(), String>(Err(format!(
                "lookback_days {} exceeds cap of {}",
                args.lookback_days, MAX_LOOKBACK_DAYS
            )));
        }
        let bar_size = match parse_bar_size(args.bar_size.as_str()) {
            Ok(b) => b,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let hist: Arc<crate::services::historical_data_service::HistoricalDataService> =
            Arc::clone(&self.historical_service);
        let result = hist
            .fetch_bars(&symbol, bar_size, Lookback::Days(args.lookback_days))
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use chrono::TimeZone;

    /// Seed `bars_cache` with five daily bars for AAPL, the most recent
    /// dated today (UTC midnight) so `compute_missing_range` returns
    /// `None` and the tool never invokes the underlying fetcher (which
    /// is the [`PanickingFetcher`] in the test harness — any cache
    /// miss would crash the test).
    #[tokio::test]
    async fn get_bars_returns_cached_daily_bars_without_fetching() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        // Seed today + 4 prior days at UTC midnight. We use the real
        // `Utc::now()` because `HistoricalDataService` uses a
        // `SystemClock` by default; matching the same instant means
        // `max_cached_day == today` and the gap-fill path is skipped.
        let today = chrono::Utc::now().date_naive();
        let mut expected_close: f64 = 0.0;
        for i in 0..5_i64 {
            let date = today - chrono::Duration::days(4 - i);
            let bar_time = chrono::Utc
                .from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                .timestamp();
            let close = 100.0 + i as f64;
            if i == 0 {
                expected_close = close;
            }
            let bar_size_str = BarSize::Day1.as_str().to_string();
            handler
                .db
                .with_conn(move |conn| {
                    conn.execute(
                        "INSERT INTO bars_cache \
                         (symbol, bar_size, bar_time, open, high, low, close, volume, wap) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        rusqlite::params![
                            "AAPL",
                            bar_size_str,
                            bar_time,
                            close - 0.5,
                            close + 1.0,
                            close - 1.0,
                            close,
                            1_000_i64 + i,
                            close,
                        ],
                    )?;
                    Ok(())
                })
                .await
                .unwrap();
        }

        let result = handler
            .get_bars(Parameters(GetBarsArgs {
                symbol: "AAPL".to_string(),
                bar_size: "1d".to_string(),
                lookback_days: 5,
            }))
            .await
            .expect("tool ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content");
        assert_eq!(body["count"].as_u64().unwrap(), 5);
        let arr = body["items"].as_array().expect("items array");
        assert_eq!(arr.len(), 5, "all five seeded bars returned");
        // Bars are sorted ascending by `bar_time`; first row is the
        // oldest seeded close (i=0 → 100.0).
        assert!(
            (arr[0]["close"].as_f64().unwrap() - expected_close).abs() < 1e-9,
            "first close = {}",
            arr[0]["close"]
        );
    }

    /// Unknown `bar_size` strings must surface as a domain error
    /// (`is_error: true` with a helpful message) rather than panicking
    /// or hitting the fetcher.
    #[tokio::test]
    async fn get_bars_unknown_bar_size_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let result = handler
            .get_bars(Parameters(GetBarsArgs {
                symbol: "AAPL".to_string(),
                bar_size: "wat".to_string(),
                lookback_days: 5,
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("unknown bar_size"), "got: {}", txt.text);
    }
}
