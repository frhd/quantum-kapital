use crate::ibkr::state::IbkrState;
use crate::ibkr::types::{ConnectionConfig, ConnectionStatus};
use tauri::State;

#[tauri::command]
pub async fn ibkr_connect(
    state: State<'_, IbkrState>,
    config: Option<ConnectionConfig>,
) -> Result<String, String> {
    // If config is provided, update the client config
    if let Some(new_config) = config {
        let _client = IbkrState::new(new_config);
        // Note: In a real implementation, you'd want to update the managed state
        // For now, we'll use the existing client
    }

    state
        .client
        .connect()
        .await
        .map(|_| "Connected successfully".to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_disconnect(state: State<'_, IbkrState>) -> Result<String, String> {
    state
        .client
        .disconnect()
        .await
        .map(|_| "Disconnected successfully".to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ibkr_get_connection_status(
    state: State<'_, IbkrState>,
) -> Result<ConnectionStatus, String> {
    state
        .client
        .get_connection_status()
        .await
        .map_err(|e| e.to_string())
}
