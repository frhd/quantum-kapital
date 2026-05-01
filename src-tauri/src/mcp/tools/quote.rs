//! `get_quote` — live IBKR market-data snapshot, never cached.
//!
//! Wraps `QuoteService::fetch_quote`, which projects a TWS
//! `MarketDataSnapshot` to the smaller UI-shaped `Quote` (last / prev
//! close / volume / timestamp). Distinct from `get_bars`: bars are
//! historical and cache-first; quotes are point-in-time and always go
//! to TWS.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetQuoteArgs {
    /// Ticker symbol (case-insensitive).
    pub symbol: String,
}

#[tool_router(router = quote_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_quote",
        description = "Return the latest live IBKR snapshot quote (last / prev close / volume) for `symbol`. Goes to TWS over the IbkrClient and never caches. Use this for current price; for historical bars use `get_bars`."
    )]
    pub async fn get_quote(
        &self,
        Parameters(args): Parameters<GetQuoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let symbol = args.symbol.trim().to_uppercase();
        if symbol.is_empty() {
            return map_tool_result::<(), &str>(Err("symbol must not be empty"));
        }
        let result = self
            .quote_service
            .fetch_quote(&symbol)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::mcp::tools::test_support::{handler_for_mock_ibkr, make_db};
    use std::sync::Arc;

    /// `MockIbkrClient::get_market_data_snapshot` returns canned values
    /// (last 150.35, close 149.80, volume 1_234_567 — see `mocks.rs`).
    /// The tool wires the mock through `QuoteService::fetch_quote`,
    /// which projects the snapshot to a `Quote`. We assert the
    /// projection is what the agent will see.
    #[tokio::test]
    async fn get_quote_returns_snapshot() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_quote(Parameters(GetQuoteArgs {
                symbol: "AAPL".to_string(),
            }))
            .await
            .expect("tool ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content");
        assert_eq!(body["symbol"].as_str().unwrap(), "AAPL");
        // serde rename_all = "camelCase" on `Quote` → `lastPrice`.
        assert!(
            (body["lastPrice"].as_f64().unwrap() - 150.35).abs() < 1e-9,
            "lastPrice = {:?}",
            body["lastPrice"]
        );
        assert!(
            (body["prevClose"].as_f64().unwrap() - 149.80).abs() < 1e-9,
            "prevClose = {:?}",
            body["prevClose"]
        );
        assert_eq!(body["volume"].as_i64().unwrap(), 1_234_567);
    }

    /// Empty / whitespace symbol must be a domain error rather than a
    /// silent IBKR call.
    #[tokio::test]
    async fn get_quote_empty_symbol_returns_domain_error() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_quote(Parameters(GetQuoteArgs {
                symbol: "   ".to_string(),
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
