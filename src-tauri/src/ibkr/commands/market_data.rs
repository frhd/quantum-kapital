use crate::ibkr::state::IbkrState;
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