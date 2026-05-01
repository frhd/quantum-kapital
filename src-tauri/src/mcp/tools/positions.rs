//! `get_positions` — current open positions for an IBKR account.
//!
//! Wraps `AccountReader::get_positions` (the narrow seam in
//! `mcp::ibkr_seam`). Production implementation routes through
//! `IbkrClient::get_positions`; tests plug a `MockIbkrClient` via the
//! same trait. The `account` arg is **optional**: when there's exactly
//! one managed account it defaults to that one; with multiple accounts
//! the call errors out and lists the choices, so the agent never picks
//! the wrong book by accident.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::{map_tool_result, resolve_account};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetPositionsArgs {
    /// IBKR account ID (e.g. `"DU123456"`). Optional. Omit to default to
    /// the connected account when only one is managed; with multiple
    /// accounts the tool errors out with the available IDs.
    #[serde(default)]
    pub account: Option<String>,
}

#[tool_router(router = positions_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_positions",
        description = "Return current open positions (symbol, quantity, average cost, market price, unrealized P&L) for an IBKR account. `account` is optional: omit it when only one IBKR account is connected; with multiple accounts the tool errors and lists the IDs so you can re-call with one. Option positions also surface `expiry`, `strike`, `right`, `multiplier` (omitted for stocks). Errors if the IBKR connection is down. Returns `{ items: [Position, ...], count: N }`."
    )]
    pub async fn get_positions(
        &self,
        Parameters(args): Parameters<GetPositionsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let account =
            match resolve_account(self.ibkr_client.as_ref(), args.account.as_deref()).await {
                Ok(a) => a,
                Err(e) => return map_tool_result::<(), String>(Err(e)),
            };
        let result = self
            .ibkr_client
            .get_positions(&account)
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
            ..Default::default()
        }
    }

    fn option_position_for(
        symbol: &str,
        qty: f64,
        account: &str,
        expiry: &str,
        strike: f64,
        right: &str,
    ) -> Position {
        Position {
            account: account.to_string(),
            symbol: symbol.to_string(),
            position: qty,
            average_cost: 1.0,
            market_price: 0.5,
            market_value: qty * 50.0,
            unrealized_pnl: -50.0,
            realized_pnl: 0.0,
            contract_type: "OPT".to_string(),
            currency: "USD".to_string(),
            exchange: "SMART".to_string(),
            // OCC-style local symbol; opaque but the structured fields
            // below are what the agent should rely on.
            local_symbol: format!("{symbol} {expiry}{right}{:08}", (strike * 1000.0) as u64),
            expiry: Some(expiry.to_string()),
            strike: Some(strike),
            right: Some(right.to_string()),
            multiplier: Some("100".to_string()),
        }
    }

    #[tokio::test]
    async fn get_positions_returns_seeded_rows() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        let seeded = vec![
            position_for("AAPL", 100.0, "DU123"),
            position_for("MSFT", 50.0, "DU123"),
        ];
        mock.set_positions(seeded.clone()).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_positions(Parameters(GetPositionsArgs {
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
        assert_eq!(arr[0]["symbol"].as_str().unwrap(), "AAPL");
        assert!((arr[0]["position"].as_f64().unwrap() - 100.0).abs() < 1e-9);
        assert_eq!(arr[1]["symbol"].as_str().unwrap(), "MSFT");
    }

    /// Single-account convenience: with one managed account, omitting
    /// `account` defaults to it instead of erroring.
    #[tokio::test]
    async fn get_positions_defaults_when_single_account() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU999".to_string()]).await;
        mock.set_positions(vec![position_for("AAPL", 7.0, "DU999")])
            .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_positions(Parameters(GetPositionsArgs { account: None }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result.structured_content.expect("structured_content");
        assert_eq!(body["count"].as_u64().unwrap(), 1);
        assert_eq!(body["items"][0]["symbol"].as_str().unwrap(), "AAPL");
    }

    /// Multi-account safety: omitting `account` must surface the
    /// available IDs rather than picking one arbitrarily.
    #[tokio::test]
    async fn get_positions_multi_account_no_arg_errors_with_choices() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU111".to_string(), "DU222".to_string()])
            .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_positions(Parameters(GetPositionsArgs { account: None }))
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

    /// Explicit account that isn't in `list_accounts` must error rather
    /// than silently fall through.
    #[tokio::test]
    async fn get_positions_unknown_account_errors() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU111".to_string()]).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_positions(Parameters(GetPositionsArgs {
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
        assert!(txt.text.contains("DU111"), "got: {}", txt.text);
    }

    /// Option positions must surface structured `expiry`, `strike`,
    /// `right`, and `multiplier` fields so the agent doesn't have to
    /// parse the OCC-encoded `local_symbol`.
    #[tokio::test]
    async fn get_positions_includes_option_contract_fields() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        mock.set_positions(vec![option_position_for(
            "BULL", 5.0, "DU123", "20250620", 10.0, "C",
        )])
        .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_positions(Parameters(GetPositionsArgs { account: None }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result.structured_content.expect("structured_content");
        let item = &body["items"][0];
        assert_eq!(item["symbol"].as_str().unwrap(), "BULL");
        assert_eq!(item["contract_type"].as_str().unwrap(), "OPT");
        assert_eq!(item["expiry"].as_str().unwrap(), "20250620");
        assert!((item["strike"].as_f64().unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(item["right"].as_str().unwrap(), "C");
        assert_eq!(item["multiplier"].as_str().unwrap(), "100");
    }

    /// Stock positions must NOT include the option-only fields in the
    /// JSON envelope (serde `skip_serializing_if = Option::is_none`).
    #[tokio::test]
    async fn get_positions_omits_option_fields_for_stocks() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        mock.set_positions(vec![position_for("AAPL", 100.0, "DU123")])
            .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_positions(Parameters(GetPositionsArgs { account: None }))
            .await
            .expect("rmcp Ok");
        let body = result.structured_content.expect("structured_content");
        let item = &body["items"][0];
        let item_obj = item.as_object().expect("item is object");
        assert!(!item_obj.contains_key("expiry"), "{item}");
        assert!(!item_obj.contains_key("strike"), "{item}");
        assert!(!item_obj.contains_key("right"), "{item}");
        assert!(!item_obj.contains_key("multiplier"), "{item}");
    }
}
