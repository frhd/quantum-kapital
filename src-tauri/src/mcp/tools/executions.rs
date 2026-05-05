//! `get_executions` — per-leg IBKR fills (executions) for an account on a
//! given trading day.
//!
//! Wraps `AccountReader::executions`. Like the sibling `get_positions` and
//! `get_account_summary`, the `account` arg is **optional**: defaults to
//! the sole managed account when one is connected; with multiple accounts
//! the tool errors and lists the IDs so an agent never silently picks the
//! wrong book. `date` defaults to the **ET trading day** at call time;
//! IBKR's `reqExecutions` endpoint only delivers fills for the current
//! TWS-day, so querying past dates returns empty (no error). Phase 4
//! persistence lifts that constraint.
//!
//! `ExecutionRow` is the wire DTO served verbatim through this tool and
//! the future Tauri `get_executions_for_date` command. It is intentionally
//! distinct from the adapter-internal `IbkrExecution` so the wire shape
//! does not drift when the IBKR adapter refactors.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::America::New_York;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::{map_tool_result, resolve_account};

/// Wire DTO for one IBKR fill.
///
/// Renames `exec_time` → `time` for readability; everything else is a
/// verbatim move from `IbkrExecution`. Option-only fields use
/// `skip_serializing_if = "Option::is_none"` so stock rows omit them in
/// the JSON envelope rather than emitting `null` — mirrors the
/// `Position` shape used by `get_positions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRow {
    pub exec_id: String,
    pub time: DateTime<Utc>,
    pub account: String,
    pub symbol: String,
    /// IBKR `secType`: `"STK"`, `"OPT"`, ...
    pub contract_type: String,
    /// Option expiry (last trading day). Omitted for non-options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<f64>,
    /// Option right normalised to `"C"` / `"P"`. Omitted for non-options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
    /// Contract multiplier (`"100"` for standard equity options). Omitted
    /// when IBKR didn't report one (typical for stocks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<String>,
    pub side: ExecutionSide,
    pub qty: f64,
    pub avg_price: f64,
    /// `None` ↔ "not (yet) reported by IBKR"; a literal `0.0` is real.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commission: Option<f64>,
    /// Realized P&L for closing legs only. `None` for opening legs or
    /// when the report has not arrived. IBKR's sign convention — gross
    /// of the closing leg's commission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realized_pnl: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commission_currency: Option<String>,
    pub order_id: i32,
    /// Phase 2: linkage to the originating setup (NULL for fills
    /// placed without a setup, including pre-P2 history).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_id: Option<i64>,
    /// Phase 2: detector-class string carried from
    /// `setups.strategy`. Same NULL semantics as `setup_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// Phase 2: absolute slippage in basis points vs the recorded
    /// intent's `intended_price`. NULL ↔ no intent matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slippage_bps: Option<i64>,
}

impl ExecutionRow {
    /// Project an IBKR adapter row into the public wire shape.
    /// Linkage fields default to `None` — populate them via the
    /// store's `query_with_linkage` for the trade-review surface.
    pub fn from_ibkr(e: IbkrExecution) -> Self {
        Self {
            exec_id: e.exec_id,
            time: e.exec_time,
            account: e.account,
            symbol: e.symbol,
            contract_type: e.contract_type,
            expiry: e.expiry,
            strike: e.strike,
            right: e.right,
            multiplier: e.multiplier,
            side: e.side,
            qty: e.qty,
            avg_price: e.avg_price,
            commission: e.commission,
            realized_pnl: e.realized_pnl,
            currency: e.currency,
            commission_currency: e.commission_currency,
            order_id: e.order_id,
            setup_id: None,
            strategy: None,
            slippage_bps: None,
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetExecutionsArgs {
    /// IBKR account ID (e.g. `"DU123456"`). Optional. Omit to default to
    /// the connected account when only one is managed; with multiple
    /// accounts the tool errors out with the available IDs.
    #[serde(default)]
    pub account: Option<String>,
    /// ISO 8601 trading day, e.g. `"2026-05-04"`, interpreted as the
    /// **ET trading day**. Optional — defaults to today (ET).
    #[serde(default)]
    pub date: Option<String>,
}

#[tool_router(router = executions_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_executions",
        description = "Returns the day's IBKR executions (fills) for `date` (defaults to today, ET trading day). Each row includes commission, realized P&L, and option contract metadata when applicable. NOTE: IBKR's reqExecutions endpoint only delivers fills for the current TWS-day; querying past dates returns an empty list. Errors if the IBKR connection is down. Returns `{ items: [ExecutionRow, ...], count: N }`."
    )]
    pub async fn get_executions(
        &self,
        Parameters(args): Parameters<GetExecutionsArgs>,
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
        let mut rows = match self.ibkr_client.executions(&account, date).await {
            Ok(r) => r,
            Err(e) => return map_tool_result::<(), String>(Err(e.to_string())),
        };
        rows.sort_by_key(|r| r.time);
        map_tool_result::<Vec<ExecutionRow>, String>(Ok(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::mcp::tools::test_support::{handler_for_mock_ibkr, make_db};
    use chrono::TimeZone;
    use std::sync::Arc;

    fn stk_exec(symbol: &str, account: &str, exec_id: &str, when: DateTime<Utc>) -> IbkrExecution {
        IbkrExecution {
            symbol: symbol.to_string(),
            side: ExecutionSide::Bought,
            qty: 100.0,
            avg_price: 150.0,
            exec_time: when,
            order_id: 1,
            exec_id: exec_id.to_string(),
            account: account.to_string(),
            contract_type: "STK".to_string(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            commission: Some(1.0),
            realized_pnl: None,
            currency: Some("USD".to_string()),
            commission_currency: Some("USD".to_string()),
        }
    }

    fn opt_exec(symbol: &str, account: &str, when: DateTime<Utc>) -> IbkrExecution {
        IbkrExecution {
            symbol: symbol.to_string(),
            side: ExecutionSide::Sold,
            qty: 1.0,
            avg_price: 5.0,
            exec_time: when,
            order_id: 7,
            exec_id: "OPT-1".to_string(),
            account: account.to_string(),
            contract_type: "OPT".to_string(),
            expiry: NaiveDate::from_ymd_opt(2026, 5, 4),
            strike: Some(390.0),
            right: Some("C".to_string()),
            multiplier: Some("100".to_string()),
            commission: Some(0.65),
            realized_pnl: Some(75.0),
            currency: Some("USD".to_string()),
            commission_currency: Some("USD".to_string()),
        }
    }

    /// Three rows with mixed UTC times within the same ET trading day must
    /// surface in ascending order — that's the day's narrative shape the
    /// FE banner and the agent both rely on.
    #[tokio::test]
    async fn get_executions_returns_canned_rows_in_chrono_order() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        // All three UTC instants land on 2026-05-04 in America/New_York
        // (EDT, UTC-4 in May): 14:30 → 10:30, 19:30 → 15:30, 20:00 → 16:00.
        let t_a = Utc.with_ymd_and_hms(2026, 5, 4, 19, 30, 0).unwrap();
        let t_b = Utc.with_ymd_and_hms(2026, 5, 4, 14, 30, 0).unwrap();
        let t_c = Utc.with_ymd_and_hms(2026, 5, 4, 20, 0, 0).unwrap();
        mock.set_executions(vec![
            stk_exec("AAPL", "DU123", "A", t_a),
            stk_exec("MSFT", "DU123", "B", t_b),
            stk_exec("TSLA", "DU123", "C", t_c),
        ])
        .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_executions(Parameters(GetExecutionsArgs {
                account: Some("DU123".to_string()),
                date: Some("2026-05-04".to_string()),
            }))
            .await
            .expect("tool ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content");
        assert_eq!(body["count"].as_u64().unwrap(), 3);
        let arr = body["items"].as_array().expect("items");
        assert_eq!(arr[0]["exec_id"].as_str().unwrap(), "B");
        assert_eq!(arr[1]["exec_id"].as_str().unwrap(), "A");
        assert_eq!(arr[2]["exec_id"].as_str().unwrap(), "C");
    }

    /// Multi-account safety: omitting `account` must surface the available
    /// IDs rather than silently pick one. Mirrors the `get_positions` test.
    #[tokio::test]
    async fn get_executions_errors_on_multi_account_without_arg() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU111".to_string(), "DU222".to_string()])
            .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_executions(Parameters(GetExecutionsArgs::default()))
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

    /// Empty days are not an error — `{items: [], count: 0}` is the
    /// committed shape. Phase 4 persistence lifts the IBKR same-day
    /// constraint; until then, querying yesterday returns the same shape.
    #[tokio::test]
    async fn get_executions_returns_empty_when_no_fills() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_executions(Parameters(GetExecutionsArgs {
                account: Some("DU123".to_string()),
                date: Some("2026-05-04".to_string()),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result.structured_content.expect("structured");
        assert_eq!(body["count"].as_u64().unwrap(), 0);
        assert!(body["items"].as_array().unwrap().is_empty());
    }

    /// Disconnected IBKR must surface as an MCP error with a recognisable
    /// message — agents and FE rely on the wording to fall back to a
    /// "connect TWS" hint instead of treating it as an empty day.
    #[tokio::test]
    async fn get_executions_errors_when_ibkr_disconnected() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        let handler = handler_for_mock_ibkr(db, Arc::clone(&mock)).await;
        // The handler builder auto-connects; flip back to disconnected to
        // exercise the NotConnected path the live tool would hit when TWS
        // is offline.
        mock.set_connected(false).await;

        let result = handler
            .get_executions(Parameters(GetExecutionsArgs {
                account: Some("DU123".to_string()),
                date: Some("2026-05-04".to_string()),
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

    /// Option fills surface structured `expiry`, `strike`, `right`,
    /// `multiplier`. Stock fills omit them entirely (skip_serializing_if).
    #[tokio::test]
    async fn get_executions_passes_option_fields_through() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        let t_stk = Utc.with_ymd_and_hms(2026, 5, 4, 14, 0, 0).unwrap();
        let t_opt = Utc.with_ymd_and_hms(2026, 5, 4, 18, 0, 0).unwrap();
        mock.set_executions(vec![
            stk_exec("AAPL", "DU123", "S1", t_stk),
            opt_exec("TSLA", "DU123", t_opt),
        ])
        .await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let result = handler
            .get_executions(Parameters(GetExecutionsArgs {
                account: Some("DU123".to_string()),
                date: Some("2026-05-04".to_string()),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(false), "{:?}", result);
        let body = result.structured_content.expect("structured");
        let items = body["items"].as_array().unwrap();
        // Sorted ascending by time: STK (14:00 UTC) first, OPT (18:00 UTC) second.
        let stk = &items[0];
        let opt = &items[1];

        assert_eq!(stk["contract_type"].as_str().unwrap(), "STK");
        let stk_obj = stk.as_object().unwrap();
        assert!(!stk_obj.contains_key("expiry"), "{stk}");
        assert!(!stk_obj.contains_key("strike"), "{stk}");
        assert!(!stk_obj.contains_key("right"), "{stk}");
        assert!(!stk_obj.contains_key("multiplier"), "{stk}");

        assert_eq!(opt["contract_type"].as_str().unwrap(), "OPT");
        assert_eq!(opt["expiry"].as_str().unwrap(), "2026-05-04");
        assert!((opt["strike"].as_f64().unwrap() - 390.0).abs() < 1e-9);
        assert_eq!(opt["right"].as_str().unwrap(), "C");
        assert_eq!(opt["multiplier"].as_str().unwrap(), "100");
        assert!((opt["realized_pnl"].as_f64().unwrap() - 75.0).abs() < 1e-9);
        assert_eq!(opt["side"].as_str().unwrap(), "sold");
    }

    /// Read-only tools must not write to `mcp_audit`. Locks the policy
    /// alongside `get_positions` and `get_account_summary`.
    #[tokio::test]
    async fn get_executions_does_not_write_audit() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        let handler = handler_for_mock_ibkr(Arc::clone(&db), mock).await;

        let _ = handler
            .get_executions(Parameters(GetExecutionsArgs {
                account: Some("DU123".to_string()),
                date: Some("2026-05-04".to_string()),
            }))
            .await
            .expect("rmcp Ok");

        let audits = crate::services::mcp_audit::list(&db, 100, 0)
            .await
            .expect("list audits");
        assert!(
            audits.is_empty(),
            "expected no mcp_audit rows, got {:?}",
            audits
        );
    }
}
