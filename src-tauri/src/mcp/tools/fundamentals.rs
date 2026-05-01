//! `get_fundamentals` — Alpha Vantage company fundamentals.
//!
//! Wraps `FinancialDataService::fetch_fundamental_data`, which already
//! applies a ~7-day on-disk cache via `cache_service.rs` and stitches
//! together AV's OVERVIEW + INCOME_STATEMENT + EARNINGS endpoints. Unlike
//! `get_news`, fundamentals failures (no API key, insufficient history,
//! transport, AV rate-limit) are surfaced to the caller as a domain
//! error: there's no shallow cache layer at the tool level, and a
//! missing fundamental is itself a meaningful signal for the agent.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFundamentalsArgs {
    /// Ticker symbol (case-insensitive).
    pub symbol: String,
}

#[tool_router(router = fundamentals_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_fundamentals",
        description = "Return Alpha Vantage company fundamentals for `symbol`: historical financials, current metrics, and analyst estimates. Locally cached for ~7 days; refreshes automatically when stale. Returns an error when the AV API key is missing or the symbol has insufficient history. Use this when reasoning about valuation, growth trajectory, or earnings-driven setups."
    )]
    pub async fn get_fundamentals(
        &self,
        Parameters(args): Parameters<GetFundamentalsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let symbol = args.symbol.trim().to_uppercase();
        if symbol.is_empty() {
            return map_tool_result::<(), &str>(Err("symbol must not be empty"));
        }
        let result = self
            .financial_service
            .fetch_fundamental_data(&symbol)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    // TODO: integration test for the happy path once we mock AV at the
    // FinancialDataService layer. AV's three-endpoint try_join makes a
    // proper mock substantial; the existing unit coverage for the
    // service lives under `services::financial_data_service::*`.

    /// `handler_for_db` builds a `FinancialDataService` with an empty AV
    /// API key; the OVERVIEW endpoint returns no usable data so
    /// `fetch_fundamental_data` errors out with the
    /// `"No historical financial data available"` message. The tool
    /// must surface this as `is_error: true` rather than swallowing it
    /// — fundamentals are heavyweight and a missing one matters.
    #[tokio::test]
    async fn get_fundamentals_surfaces_av_failure_as_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let result = handler
            .get_fundamentals(Parameters(GetFundamentalsArgs {
                symbol: "AAPL".to_string(),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text content present");
        assert!(!txt.text.is_empty(), "error text must be non-empty");
    }

    #[tokio::test]
    async fn get_fundamentals_empty_symbol_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let result = handler
            .get_fundamentals(Parameters(GetFundamentalsArgs {
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
