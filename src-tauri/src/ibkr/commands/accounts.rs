use crate::ibkr::state::IbkrState;
use crate::ibkr::types::{AccountSummary, Position};
use tauri::State;

#[tauri::command]
pub async fn ibkr_get_accounts(state: State<'_, IbkrState>) -> Result<Vec<String>, String> {
    state.client.get_accounts().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_get_account_summary(
    state: State<'_, IbkrState>,
    account: String,
) -> Result<Vec<AccountSummary>, String> {
    state
        .client
        .get_account_summary(&account)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_get_positions(state: State<'_, IbkrState>) -> Result<Vec<Position>, String> {
    state
        .client
        .get_positions()
        .await
        .map_err(|e| e.to_string())
}
