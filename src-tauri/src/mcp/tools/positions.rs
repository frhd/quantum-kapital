//! `get_positions` — current open positions for an IBKR account.
//!
//! Wraps `AccountReader::get_positions` (the narrow seam in
//! `mcp::ibkr_seam`). Production implementation routes through
//! `IbkrClient::get_positions` (which queries TWS's connected account);
//! tests plug a `MockIbkrClient` via the same trait. The `account` arg
//! is REQUIRED — silently defaulting to "the first account" would
//! mislead the agent when the user has multiple IBKR accounts.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPositionsArgs {
    /// IBKR account ID (e.g. `"DU123456"`). Required — see module docs
    /// on why we don't default.
    pub account: String,
}

#[tool_router(router = positions_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_positions",
        description = "Return current open positions (symbol, quantity, average cost, market price, unrealized P&L) for the given IBKR `account`. Use this when reasoning about portfolio composition, exposure, or whether a setup overlaps an existing position. Errors if the IBKR connection is down."
    )]
    pub async fn get_positions(
        &self,
        Parameters(args): Parameters<GetPositionsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let account = args.account.trim();
        if account.is_empty() {
            return map_tool_result::<(), &str>(Err("account must not be empty"));
        }
        let result = self
            .ibkr_client
            .get_positions(account)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::ibkr::types::Position;
    use crate::mcp::tools::test_support::{handler_for_mock_ibkr, make_db};
    use std::sync::Arc;

    fn position_for(symbol: &str, qty: f64, account: &str) -> Position {
        Position {
            account: account.to_string(),
            symbol: symbol.to_string(),
            position: qty,
            average_cost: 100.0,
            market_price: 110.0,
            market_value: qty * 110.0,
            unrealized_pnl: qty * 10.0,
            realized_pnl: 0.0,
            contract_type: "STK".to_string(),
            currency: "USD".to_string(),
            exchange: "NASDAQ".to_string(),
            local_symbol: symbol.to_string(),
        }
    }

    #[tokio::test]
    async fn get_positions_returns_seeded_rows() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let seeded = vec![
            position_for("AAPL", 100.0, "DU123"),
            position_for("MSFT", 50.0, "DU123"),
        ];
        mock.set_positions(seeded.clone()).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_positions(Parameters(GetPositionsArgs {
                account: "DU123".to_string(),
            }))
            .await
            .expect("tool ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let arr = result
            .structured_content
            .as_ref()
            .expect("structured_content")
            .as_array()
            .expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["symbol"].as_str().unwrap(), "AAPL");
        assert!((arr[0]["position"].as_f64().unwrap() - 100.0).abs() < 1e-9);
        assert_eq!(arr[1]["symbol"].as_str().unwrap(), "MSFT");
    }

    #[tokio::test]
    async fn get_positions_empty_account_returns_domain_error() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_positions(Parameters(GetPositionsArgs {
                account: "  ".to_string(),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("account"), "got: {}", txt.text);
    }
}
