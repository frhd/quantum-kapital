use tauri::State;
use crate::ibkr::state::IbkrState;
use crate::ibkr::types::*;

#[tauri::command]
pub async fn ibkr_connect(
    state: State<'_, IbkrState>,
    config: Option<ConnectionConfig>
) -> Result<String, String> {
    // If config is provided, update the client config
    if let Some(new_config) = config {
        let _client = IbkrState::new(new_config);
        // Note: In a real implementation, you'd want to update the managed state
        // For now, we'll use the existing client
    }
    
    state.client.connect().await
        .map(|_| "Connected successfully".to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_disconnect(state: State<'_, IbkrState>) -> Result<String, String> {
    state.client.disconnect().await
        .map(|_| "Disconnected successfully".to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_get_connection_status(
    state: State<'_, IbkrState>
) -> Result<ConnectionStatus, String> {
    state.client.get_connection_status().await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_get_accounts(state: State<'_, IbkrState>) -> Result<Vec<String>, String> {
    state.client.get_accounts().await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_get_account_summary(
    state: State<'_, IbkrState>,
    account: String
) -> Result<Vec<AccountSummary>, String> {
    state.client.get_account_summary(&account).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_get_positions(state: State<'_, IbkrState>) -> Result<Vec<Position>, String> {
    state.client.get_positions().await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_subscribe_market_data(
    state: State<'_, IbkrState>,
    symbol: String
) -> Result<String, String> {
    state.client.subscribe_market_data(&symbol).await
        .map(|_| format!("Subscribed to market data for {}", symbol))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_place_order(
    state: State<'_, IbkrState>,
    order: OrderRequest
) -> Result<i32, String> {
    state.client.place_order(order).await
        .map_err(|e| e.to_string())
}