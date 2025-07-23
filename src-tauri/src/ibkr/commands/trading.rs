use crate::ibkr::state::IbkrState;
use crate::ibkr::types::OrderRequest;
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
