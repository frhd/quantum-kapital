//! `get_account_summary` — account-level metrics (NetLiquidation,
//! BuyingPower, …) for an IBKR account.
//!
//! Wraps `AccountReader::get_account_summary`. Like `get_positions` the
//! `account` arg is REQUIRED — silently picking "the first account"
//! would mislead the agent in multi-account setups.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAccountSummaryArgs {
    /// IBKR account ID (e.g. `"DU123456"`). Required.
    pub account: String,
}

#[tool_router(router = account_summary_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_account_summary",
        description = "Return account-level summary metrics (NetLiquidation, BuyingPower, TotalCashValue, etc.) for IBKR `account`. Each row carries a `tag` (the metric name), `value` (string-encoded number), and `currency`. Use this to gauge available headroom before sizing a position. Errors if the IBKR connection is down."
    )]
    pub async fn get_account_summary(
        &self,
        Parameters(args): Parameters<GetAccountSummaryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let account = args.account.trim();
        if account.is_empty() {
            return map_tool_result::<(), &str>(Err("account must not be empty"));
        }
        let result = self
            .ibkr_client
            .get_account_summary(account)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::ibkr::types::AccountSummary;
    use crate::mcp::tools::test_support::{handler_for_mock_ibkr, make_db};
    use std::sync::Arc;

    fn summary_row(account: &str, tag: &str, value: &str) -> AccountSummary {
        AccountSummary {
            account: account.to_string(),
            tag: tag.to_string(),
            value: value.to_string(),
            currency: "USD".to_string(),
        }
    }

    #[tokio::test]
    async fn get_account_summary_returns_seeded_rows() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let seeded = vec![
            summary_row("DU123", "NetLiquidation", "100000.0"),
            summary_row("DU123", "BuyingPower", "200000.0"),
        ];
        mock.set_account_summary(seeded.clone()).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_account_summary(Parameters(GetAccountSummaryArgs {
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
        assert_eq!(arr[0]["tag"].as_str().unwrap(), "NetLiquidation");
        assert_eq!(arr[0]["value"].as_str().unwrap(), "100000.0");
        assert_eq!(arr[1]["tag"].as_str().unwrap(), "BuyingPower");
    }

    #[tokio::test]
    async fn get_account_summary_empty_account_returns_domain_error() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_account_summary(Parameters(GetAccountSummaryArgs {
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
