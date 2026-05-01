//! `get_account_summary` — account-level metrics (NetLiquidation,
//! BuyingPower, …) for an IBKR account.
//!
//! Wraps `AccountReader::get_account_summary`. Like `get_positions` the
//! `account` arg is **optional**: defaults to the sole managed account
//! when one is connected; errors and lists the choices when there are
//! multiple, so the agent never silently picks the wrong book.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::{map_tool_result, resolve_account};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetAccountSummaryArgs {
    /// IBKR account ID (e.g. `"DU123456"`). Optional. Omit to default to
    /// the connected account when only one is managed; with multiple
    /// accounts the tool errors out with the available IDs.
    #[serde(default)]
    pub account: Option<String>,
}

#[tool_router(router = account_summary_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_account_summary",
        description = "Return account-level summary metrics (NetLiquidation, BuyingPower, TotalCashValue, etc.) for an IBKR account. `account` is optional: omit it when only one IBKR account is connected; with multiple accounts the tool errors and lists the IDs so you can re-call with one. Each row carries a `tag` (metric name), `value` (string-encoded number), and `currency`. Errors if the IBKR connection is down. Returns `{ items: [AccountSummary, ...], count: N }`."
    )]
    pub async fn get_account_summary(
        &self,
        Parameters(args): Parameters<GetAccountSummaryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let account =
            match resolve_account(self.ibkr_client.as_ref(), args.account.as_deref()).await {
                Ok(a) => a,
                Err(e) => return map_tool_result::<(), String>(Err(e)),
            };
        let result = self
            .ibkr_client
            .get_account_summary(&account)
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
        mock.set_accounts(vec!["DU123".to_string()]).await;
        let seeded = vec![
            summary_row("DU123", "NetLiquidation", "100000.0"),
            summary_row("DU123", "BuyingPower", "200000.0"),
        ];
        mock.set_account_summary(seeded.clone()).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_account_summary(Parameters(GetAccountSummaryArgs {
                account: Some("DU123".to_string()),
            }))
            .await
            .expect("tool ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content");
        assert_eq!(body["count"].as_u64().unwrap(), 2);
        let arr = body["items"].as_array().expect("items array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["tag"].as_str().unwrap(), "NetLiquidation");
        assert_eq!(arr[0]["value"].as_str().unwrap(), "100000.0");
        assert_eq!(arr[1]["tag"].as_str().unwrap(), "BuyingPower");
    }

    /// Single-account convenience: omitting `account` defaults to it.
    #[tokio::test]
    async fn get_account_summary_defaults_when_single_account() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU999".to_string()]).await;
        mock.set_account_summary(vec![summary_row("DU999", "NetLiquidation", "42.0")])
            .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_account_summary(Parameters(GetAccountSummaryArgs { account: None }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result.structured_content.expect("structured_content");
        assert_eq!(body["count"].as_u64().unwrap(), 1);
    }

    /// Multi-account safety: must surface available IDs.
    #[tokio::test]
    async fn get_account_summary_multi_account_no_arg_errors_with_choices() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU111".to_string(), "DU222".to_string()])
            .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_account_summary(Parameters(GetAccountSummaryArgs { account: None }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("DU111"), "got: {}", txt.text);
        assert!(txt.text.contains("DU222"), "got: {}", txt.text);
    }

    /// Unknown explicit account must error.
    #[tokio::test]
    async fn get_account_summary_unknown_account_errors() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU111".to_string()]).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_account_summary(Parameters(GetAccountSummaryArgs {
                account: Some("DU999".to_string()),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("DU999"), "got: {}", txt.text);
    }
}
