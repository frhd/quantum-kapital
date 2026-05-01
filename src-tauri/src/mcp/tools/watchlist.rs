//! `get_watchlist` — read the tracker watchlist, optionally filtered by status.
//!
//! Wraps `TrackerService::list(Option<TrackerStatus>)`. The status filter
//! takes the same snake_case strings the rest of the app uses; an unknown
//! string surfaces as a domain error via [`map_tool_result`] so the agent
//! can fix it without the JSON-RPC layer faulting.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::ibkr::types::tracker::TrackerStatus;
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;

/// Parameters for `get_watchlist`. `status` is the optional snake_case
/// filter — omit (or pass `null`) to get every active tracked ticker.
#[derive(Debug, Deserialize, JsonSchema, Default)]
pub struct GetWatchlistArgs {
    /// One of `"watching"`, `"in_play"`, `"setup_active"`, `"cool_down"`.
    /// Omit to return every non-archived row.
    #[serde(default)]
    pub status: Option<String>,
}

#[tool_router(router = watchlist_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_watchlist",
        description = "List tracked tickers from the tracker watchlist, newest-added first. Optional `status` filter accepts \"watching\", \"in_play\", \"setup_active\", or \"cool_down\". Use this to ground research about which symbols the user is currently surveilling, before reaching for live quotes or scans."
    )]
    pub async fn get_watchlist(
        &self,
        Parameters(args): Parameters<GetWatchlistArgs>,
    ) -> Result<CallToolResult, McpError> {
        let status = match args.status.as_deref() {
            None => None,
            Some(s) => match TrackerStatus::parse(s) {
                Some(parsed) => Some(parsed),
                None => {
                    return map_tool_result::<(), String>(Err(format!(
                        "unknown tracker status: {s}"
                    )));
                }
            },
        };
        let result = self.tracker.list(status).await.map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::tracker::{StrategyTag, TrackerSource};
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    /// Seeds two rows with different statuses and asserts the filter
    /// flows through to the SQL layer; no filter returns both, an
    /// `in_play` filter returns just the one row.
    #[tokio::test]
    async fn get_watchlist_no_filter_returns_all_rows() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        handler
            .tracker
            .add(
                "AAPL",
                TrackerSource::Manual,
                None,
                vec![StrategyTag::Breakout],
                None,
            )
            .await
            .unwrap();
        handler
            .tracker
            .add("TSLA", TrackerSource::Scanner, None, vec![], None)
            .await
            .unwrap();

        let result = handler
            .get_watchlist(Parameters(GetWatchlistArgs { status: None }))
            .await
            .expect("tool ok");
        assert_eq!(result.is_error, Some(false));
        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content");
        let arr = body.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        let symbols: Vec<&str> = arr.iter().map(|r| r["symbol"].as_str().unwrap()).collect();
        assert!(symbols.contains(&"AAPL"));
        assert!(symbols.contains(&"TSLA"));
    }

    #[tokio::test]
    async fn get_watchlist_with_status_filter_narrows_rows() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        handler
            .tracker
            .add("AAPL", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        handler
            .tracker
            .set_status(
                "AAPL",
                crate::ibkr::types::tracker::TrackerStatus::InPlay,
                None,
                None,
            )
            .await
            .unwrap();
        handler
            .tracker
            .add("TSLA", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();

        let result = handler
            .get_watchlist(Parameters(GetWatchlistArgs {
                status: Some("in_play".to_string()),
            }))
            .await
            .expect("tool ok");
        let arr = result
            .structured_content
            .as_ref()
            .expect("structured_content")
            .as_array()
            .expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["symbol"].as_str().unwrap(), "AAPL");
        assert_eq!(arr[0]["status"].as_str().unwrap(), "in_play");
    }

    #[tokio::test]
    async fn get_watchlist_unknown_status_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let result = handler
            .get_watchlist(Parameters(GetWatchlistArgs {
                status: Some("nonexistent".to_string()),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text content");
        assert!(txt.text.contains("nonexistent"), "got: {}", txt.text);
    }
}
