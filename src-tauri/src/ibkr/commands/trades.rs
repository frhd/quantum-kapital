//! Phase 3 — `ibkr_get_executions_for_date` Tauri command.
//!
//! The frontend's path to the same `AccountReader::executions` seam the
//! `get_executions` MCP tool uses, so the desktop UI and a Claude Code
//! agent see byte-identical rows. Returns the wire-stable
//! [`ExecutionRow`] DTO (defined in `mcp/tools/executions.rs`) — never
//! the adapter-internal `IbkrExecution`.
//!
//! Account resolution mirrors the MCP tool: optional `account` arg
//! defaults to the sole managed account; multi-account without an
//! explicit choice surfaces the available IDs so the caller can re-issue
//! with one. IBKR's `reqExecutions` only returns the current TWS-day,
//! so prior dates render as an empty list (Phase 4 lifts that
//! constraint).

use chrono::NaiveDate;
use tauri::State;

use crate::ibkr::state::IbkrState;
use crate::mcp::ibkr_seam::AccountReader;
use crate::mcp::tools::executions::ExecutionRow;
use crate::mcp::tools::resolve_account;

/// Shared core so the command logic is unit-testable against the
/// `AccountReader` seam without spinning up a Tauri `State`.
pub(crate) async fn fetch_executions_for_date(
    reader: &dyn AccountReader,
    account: Option<&str>,
    date: &str,
) -> Result<Vec<ExecutionRow>, String> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("invalid date '{date}', expected YYYY-MM-DD: {e}"))?;
    let resolved = resolve_account(reader, account).await?;
    let mut rows = reader
        .executions(&resolved, parsed)
        .await
        .map_err(|e| e.to_string())?;
    rows.sort_by_key(|r| r.time);
    Ok(rows)
}

#[tauri::command]
pub async fn ibkr_get_executions_for_date(
    state: State<'_, IbkrState>,
    account: Option<String>,
    date: String,
) -> Result<Vec<ExecutionRow>, String> {
    fetch_executions_for_date(state.client.as_ref(), account.as_deref(), &date).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::ibkr::types::{ExecutionSide, IbkrExecution};
    use chrono::{DateTime, TimeZone, Utc};
    use std::sync::Arc;

    fn stk(symbol: &str, account: &str, exec_id: &str, when: DateTime<Utc>) -> IbkrExecution {
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

    #[tokio::test]
    async fn fetch_returns_rows_sorted_by_time_for_resolved_account() {
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        mock.set_connected(true).await;
        let t_a = Utc.with_ymd_and_hms(2026, 5, 4, 19, 30, 0).unwrap();
        let t_b = Utc.with_ymd_and_hms(2026, 5, 4, 14, 30, 0).unwrap();
        mock.set_executions(vec![
            stk("AAPL", "DU123", "A", t_a),
            stk("MSFT", "DU123", "B", t_b),
        ])
        .await;

        let reader: &dyn AccountReader = mock.as_ref();
        let rows = fetch_executions_for_date(reader, None, "2026-05-04")
            .await
            .expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].exec_id, "B");
        assert_eq!(rows[1].exec_id, "A");
    }

    #[tokio::test]
    async fn fetch_errors_on_invalid_date() {
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        mock.set_connected(true).await;
        let reader: &dyn AccountReader = mock.as_ref();
        let err = fetch_executions_for_date(reader, None, "2026/05/04")
            .await
            .expect_err("must reject non-ISO date");
        assert!(err.contains("YYYY-MM-DD"), "got: {err}");
    }

    #[tokio::test]
    async fn fetch_errors_on_multi_account_without_arg() {
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU111".to_string(), "DU222".to_string()])
            .await;
        mock.set_connected(true).await;
        let reader: &dyn AccountReader = mock.as_ref();
        let err = fetch_executions_for_date(reader, None, "2026-05-04")
            .await
            .expect_err("multi-account without arg");
        assert!(err.contains("DU111") && err.contains("DU222"), "got: {err}");
    }

    #[tokio::test]
    async fn fetch_returns_empty_when_no_fills() {
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["DU123".to_string()]).await;
        mock.set_connected(true).await;
        let reader: &dyn AccountReader = mock.as_ref();
        let rows = fetch_executions_for_date(reader, Some("DU123"), "2026-05-04")
            .await
            .expect("ok");
        assert!(rows.is_empty());
    }
}
