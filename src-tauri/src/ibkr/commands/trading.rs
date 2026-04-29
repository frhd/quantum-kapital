use crate::ibkr::state::IbkrState;
use crate::ibkr::types::{IbkrExecution, OrderRequest};
use chrono::NaiveDate;
use tauri::State;

#[tauri::command]
pub async fn ibkr_place_order(
    state: State<'_, IbkrState>,
    order: OrderRequest,
) -> Result<i32, String> {
    state
        .client
        .place_order(order)
        .await
        .map_err(|e| e.to_string())
}

/// Parses a `YYYY-MM-DD` string into a `NaiveDate`, returning a typed error
/// for the Tauri command boundary. Extracted so it can be unit-tested without
/// constructing a Tauri `State`.
pub(crate) fn parse_date_arg(date: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("invalid date '{date}', expected YYYY-MM-DD: {e}"))
}

#[tauri::command]
pub async fn ibkr_get_executions(
    state: State<'_, IbkrState>,
    date: String,
) -> Result<Vec<IbkrExecution>, String> {
    let parsed = parse_date_arg(&date)?;
    state
        .client
        .executions(parsed)
        .await
        .map_err(|e| e.to_string())
}
