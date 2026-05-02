//! `get_fundamentals` — company fundamentals via the
//! [`FundamentalsProvider`] trait.
//!
//! Phase 3 wires the AV-backed adapter directly; Phase 4 layers a
//! manual store on top via the composite provider, but this file does
//! not change because the trait surface is the contract. Unlike
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
        description = "Return company fundamentals for `symbol`: historical financials, current metrics, and analyst estimates. Returns an error when the upstream provider has no data (missing API key, insufficient history, rate-limit). Use this when reasoning about valuation, growth trajectory, or earnings-driven setups."
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
            .fundamentals_provider
            .fetch(&symbol)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    // The Phase 3 trait swap means the happy path is exercised in the
    // provider-level tests at `services::fundamentals_provider::tests`
    // (`av_adapter_round_trips_canned_payloads_into_fundamental_data`).
    // The MCP tool itself is now thin enough that all it needs to prove
    // is "errors flow through as domain errors, not protocol faults".

    /// `handler_for_db` wires a [`FakeFundamentalsProvider`] with no
    /// rows; every `fetch` returns [`FundamentalsError::NotFound`],
    /// which the tool surfaces via `map_tool_result` as
    /// `is_error: true` with a non-empty message. This guards against
    /// regressions that swallow upstream errors back into a successful
    /// MCP reply (the pre-Phase-3 silent mock-data fallback).
    #[tokio::test]
    async fn get_fundamentals_surfaces_provider_failure_as_domain_error() {
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
