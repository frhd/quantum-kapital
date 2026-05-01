use crate::ibkr::state::IbkrState;
use crate::ibkr::types::DataTier;
use tauri::State;

#[tauri::command]
pub async fn ibkr_subscribe_market_data(
    state: State<'_, IbkrState>,
    symbol: String,
) -> Result<String, String> {
    state
        .client
        .subscribe_market_data(&symbol)
        .await
        .map(|_| format!("Subscribed to market data for {symbol}"))
        .map_err(|e| e.to_string())
}

/// Read the empirically detected market-data tier for the active
/// connection. Returns `DataTier::Unknown` before the connect-time
/// probe completes, after a disconnect, or while disconnected. The
/// frontend uses this to hydrate state on tab switch / page reload
/// instead of waiting for the next `data-tier-detected` event.
#[tauri::command]
pub async fn ibkr_get_data_tier(state: State<'_, IbkrState>) -> Result<DataTier, String> {
    Ok(*state.data_tier.read().await)
}
