use crate::ibkr::state::IbkrState;
use crate::ibkr::types::ScannerSubscription;
use tauri::State;

#[tauri::command]
pub async fn ibkr_start_scanner(
    state: State<'_, IbkrState>,
    subscription: ScannerSubscription,
) -> Result<(), String> {
    state.start_scanner(subscription).await
}

#[tauri::command]
pub async fn ibkr_stop_scanner(state: State<'_, IbkrState>) -> Result<(), String> {
    state.stop_scanner().await;
    Ok(())
}
