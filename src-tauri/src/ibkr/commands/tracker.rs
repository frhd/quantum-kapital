use std::sync::Arc;
use tauri::State;

use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::services::historical_data_service::{HistoricalDataService, Lookback};

#[tauri::command]
pub async fn tracker_fetch_bars(
    service: State<'_, Arc<HistoricalDataService>>,
    symbol: String,
    bar_size: BarSize,
    lookback_days: u32,
) -> Result<Vec<HistoricalBar>, String> {
    service
        .fetch_bars(&symbol, bar_size, Lookback::Days(lookback_days))
        .await
        .map_err(|e| e.to_string())
}
