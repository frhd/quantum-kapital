//! `get_trade_legs` — FIFO leg-matched view of fills for a date.
//!
//! Calls `AccountReader::executions` (transparently served from the
//! Phase 1 store for past days, live IBKR for today), then runs the
//! pure FIFO matcher (`services::trade_legs`) to produce round-trip +
//! carryover legs with per-leg net P&L. Replaces the manual leg-grouping
//! arithmetic LLM clients had to do by hand against `get_executions`.

use chrono::{NaiveDate, Utc};
use chrono_tz::America::New_York;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::{map_tool_result, resolve_account};
use crate::services::trade_legs::{compute_totals, match_legs};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetTradeLegsArgs {
    /// IBKR account ID. Optional — defaults to the sole managed account.
    #[serde(default)]
    pub account: Option<String>,
    /// ISO 8601 ET trading day, e.g. `"2026-05-04"`. Optional — defaults to today (ET).
    #[serde(default)]
    pub date: Option<String>,
    /// Optional symbol filter (case-insensitive).
    #[serde(default)]
    pub symbol: Option<String>,
}

#[tool_router(router = get_trade_legs_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_trade_legs",
        description = "FIFO leg-matched view of fills for `date` (defaults to today, ET trading day). Groups buys+sells per (symbol, contract_type, expiry, strike, right) into round-trip legs with realized P&L net of commissions; emits one carryover leg per unclosed open. Returns `{ date, account, legs: [TradeLeg, ...], totals: { gross_pnl, net_pnl, commissions, n_round_trips, n_carryover, by_symbol } }`. Past dates are served from the executions store; current day is fresh from IBKR. Errors if the IBKR connection is down for today's date."
    )]
    pub async fn get_trade_legs(
        &self,
        Parameters(args): Parameters<GetTradeLegsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let date = match args
            .date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                Ok(d) => d,
                Err(e) => {
                    return map_tool_result::<(), String>(Err(format!(
                        "invalid `date` (expected YYYY-MM-DD): {e}"
                    )));
                }
            },
            None => Utc::now().with_timezone(&New_York).date_naive(),
        };
        let account =
            match resolve_account(self.ibkr_client.as_ref(), args.account.as_deref()).await {
                Ok(a) => a,
                Err(e) => return map_tool_result::<(), String>(Err(e)),
            };
        let mut fills = match self.ibkr_client.executions(&account, date).await {
            Ok(r) => r,
            Err(e) => return map_tool_result::<(), String>(Err(e.to_string())),
        };
        if let Some(filter) = args.symbol.as_deref() {
            let needle = filter.trim();
            if !needle.is_empty() {
                fills.retain(|f| f.symbol.eq_ignore_ascii_case(needle));
            }
        }
        let legs = match_legs(&fills);
        let totals = compute_totals(&legs);
        map_tool_result::<_, String>(Ok(json!({
            "date": date.to_string(),
            "account": account,
            "legs": legs,
            "totals": totals,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::ibkr::types::{ExecutionSide, IbkrExecution};
    use crate::mcp::tools::test_support::{handler_for_mock_ibkr, make_db};
    use chrono::{DateTime, TimeZone};
    use std::sync::Arc;

    fn opt_exec(
        exec_id: &str,
        side: ExecutionSide,
        qty: f64,
        price: f64,
        when: DateTime<Utc>,
    ) -> IbkrExecution {
        IbkrExecution {
            symbol: "TSLA".to_string(),
            side,
            qty,
            avg_price: price,
            exec_time: when,
            order_id: 1,
            exec_id: exec_id.to_string(),
            account: "U1".to_string(),
            contract_type: "OPT".to_string(),
            expiry: NaiveDate::from_ymd_opt(2026, 5, 4),
            strike: Some(395.0),
            right: Some("C".to_string()),
            multiplier: Some("100".to_string()),
            commission: Some(0.50),
            realized_pnl: None,
            currency: Some("USD".to_string()),
            commission_currency: Some("USD".to_string()),
        }
    }

    /// Tracer test: round-trip on TSLA 395C produces one closed leg with
    /// net P&L matching the spec's worked example.
    #[tokio::test]
    async fn get_trade_legs_returns_round_trip_legs() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["U1".to_string()]).await;
        let t = |min: u32| Utc.with_ymd_and_hms(2026, 5, 4, 17, min, 0).unwrap();
        mock.set_executions(vec![
            opt_exec("OPEN", ExecutionSide::Bought, 3.0, 1.50, t(32)),
            opt_exec("CLOSE", ExecutionSide::Sold, 3.0, 2.45, t(42)),
        ])
        .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_trade_legs(Parameters(GetTradeLegsArgs {
                account: Some("U1".into()),
                date: Some("2026-05-04".into()),
                symbol: None,
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result.structured_content.expect("structured");
        assert_eq!(body["date"], "2026-05-04");
        assert_eq!(body["account"], "U1");
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0]["symbol"], "TSLA");
        let totals = &body["totals"];
        assert_eq!(totals["n_round_trips"].as_u64().unwrap(), 1);
        assert_eq!(totals["n_carryover"].as_u64().unwrap(), 0);
        assert!((totals["gross_pnl"].as_f64().unwrap() - 285.0).abs() < 1e-6);
        assert!((totals["net_pnl"].as_f64().unwrap() - 284.0).abs() < 1e-6);
        assert!((totals["commissions"].as_f64().unwrap() - 1.0).abs() < 1e-6);
    }

    /// Symbol filter narrows the fills before matching.
    #[tokio::test]
    async fn get_trade_legs_symbol_filter_narrows_fills() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["U1".to_string()]).await;
        let t = |min: u32| Utc.with_ymd_and_hms(2026, 5, 4, 17, min, 0).unwrap();
        let mut other = opt_exec("X1", ExecutionSide::Bought, 1.0, 1.0, t(20));
        other.symbol = "AAPL".to_string();
        let mut other_close = opt_exec("X2", ExecutionSide::Sold, 1.0, 2.0, t(30));
        other_close.symbol = "AAPL".to_string();
        mock.set_executions(vec![
            opt_exec("OPEN", ExecutionSide::Bought, 3.0, 1.50, t(32)),
            opt_exec("CLOSE", ExecutionSide::Sold, 3.0, 2.45, t(42)),
            other,
            other_close,
        ])
        .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_trade_legs(Parameters(GetTradeLegsArgs {
                account: Some("U1".into()),
                date: Some("2026-05-04".into()),
                symbol: Some("tsla".into()),
            }))
            .await
            .expect("rmcp Ok");
        let body = result.structured_content.expect("structured");
        let legs = body["legs"].as_array().unwrap();
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0]["symbol"], "TSLA");
    }

    /// Empty days are not an error — `{legs: [], totals: {...zeros}}`.
    #[tokio::test]
    async fn get_trade_legs_returns_empty_when_no_fills() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["U1".to_string()]).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_trade_legs(Parameters(GetTradeLegsArgs {
                account: Some("U1".into()),
                date: Some("2026-05-04".into()),
                symbol: None,
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result.structured_content.expect("structured");
        assert!(body["legs"].as_array().unwrap().is_empty());
        assert_eq!(body["totals"]["n_round_trips"].as_u64().unwrap(), 0);
        assert_eq!(body["totals"]["n_carryover"].as_u64().unwrap(), 0);
    }

    /// Disconnected IBKR surfaces as MCP error.
    #[tokio::test]
    async fn get_trade_legs_errors_when_ibkr_disconnected() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["U1".into()]).await;
        let handler = handler_for_mock_ibkr(db, Arc::clone(&mock)).await;
        mock.set_connected(false).await;

        let result = handler
            .get_trade_legs(Parameters(GetTradeLegsArgs {
                account: Some("U1".into()),
                date: Some("2026-05-04".into()),
                symbol: None,
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(
            txt.text.to_lowercase().contains("not connected"),
            "got: {}",
            txt.text
        );
    }

    /// Read-only audit invariant — the tool must NOT write `mcp_audit` rows.
    #[tokio::test]
    async fn get_trade_legs_does_not_write_audit() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["U1".into()]).await;
        let handler = handler_for_mock_ibkr(Arc::clone(&db), mock).await;

        let _ = handler
            .get_trade_legs(Parameters(GetTradeLegsArgs {
                account: Some("U1".into()),
                date: Some("2026-05-04".into()),
                symbol: None,
            }))
            .await
            .expect("rmcp Ok");

        let audits = crate::services::mcp_audit::list(&db, 100, 0)
            .await
            .expect("list");
        assert!(audits.is_empty(), "got {:?}", audits);
    }
}
