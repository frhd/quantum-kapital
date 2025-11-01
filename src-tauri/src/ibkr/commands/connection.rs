use crate::ibkr::state::IbkrState;
use crate::ibkr::types::{ConnectionConfig, ConnectionStatus};
use tauri::State;

#[tauri::command]
pub async fn ibkr_connect(
    state: State<'_, IbkrState>,
    config: Option<ConnectionConfig>,
) -> Result<String, String> {
    tracing::info!("🟢 CONNECT COMMAND CALLED");

    // If config is provided, update the client config
    if let Some(new_config) = config {
        let _client = IbkrState::new(new_config);
        // Note: In a real implementation, you'd want to update the managed state
        // For now, we'll use the existing client
    }

    let result = state.client.connect().await;
    tracing::info!("🟢 CONNECT RESULT: {:?}", result.is_ok());

    match result {
        Ok(_) => {
            tracing::info!("🟢 UPDATING CONNECTION STATUS TO TRUE");
            state.update_connection_status(true).await;
            tracing::info!("🟢 CONNECT SUCCESSFUL - RETURNING SUCCESS");
            Ok("Connected successfully".to_string())
        }
        Err(e) => {
            tracing::error!("🟢 CONNECT ERROR: {}", e);
            state.update_connection_status(false).await;
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn ibkr_disconnect(state: State<'_, IbkrState>) -> Result<String, String> {
    tracing::info!("🔴 DISCONNECT COMMAND CALLED");

    let result = state.client.disconnect().await;
    tracing::info!("🔴 DISCONNECT RESULT: {:?}", result.is_ok());

    match result {
        Ok(_) => {
            tracing::info!("🔴 UPDATING CONNECTION STATUS TO FALSE");
            state.update_connection_status(false).await;

            // Increment client ID to avoid conflicts on reconnect
            state.increment_client_id().await;

            tracing::info!("🔴 DISCONNECT SUCCESSFUL - RETURNING SUCCESS");
            Ok("Disconnected successfully".to_string())
        }
        Err(e) => {
            tracing::error!("🔴 DISCONNECT ERROR: {}", e);
            Err(e.to_string())
        }
    }
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
