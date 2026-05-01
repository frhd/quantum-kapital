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
pub async fn ibkr_get_positions(
    state: State<'_, IbkrState>,
    account: Option<String>,
) -> Result<Vec<Position>, String> {
    // Resolve which account to query: explicit arg wins, otherwise fall
    // back to the first managed account (preserves the pre-existing UI
    // behaviour where the command took no args).
    let account = match account {
        Some(a) if !a.trim().is_empty() => a,
        _ => {
            let accounts = state
                .client
                .get_accounts()
                .await
                .map_err(|e| e.to_string())?;
            accounts
                .into_iter()
                .next()
                .ok_or_else(|| "no IBKR accounts available".to_string())?
        }
    };
    state
        .client
        .get_positions(&account)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_start_daily_pnl(
    state: State<'_, IbkrState>,
    account: String,
) -> Result<(), String> {
    state.start_daily_pnl(&account).await
}

#[tauri::command]
pub async fn ibkr_stop_daily_pnl(state: State<'_, IbkrState>) -> Result<(), String> {
    state.stop_daily_pnl().await;
    Ok(())
}
