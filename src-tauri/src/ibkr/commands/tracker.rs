use std::sync::Arc;
use tauri::State;

use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::news::NewsItem;
use crate::services::financial_data_service::FinancialDataService;
use crate::services::historical_data_service::{HistoricalDataService, Lookback};
use crate::storage::Db;

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

#[tauri::command]
pub async fn tracker_get_news(
    db: State<'_, Arc<Db>>,
    symbol: String,
    lookback_hours: u32,
) -> Result<Vec<NewsItem>, String> {
    let api_key = std::env::var("ALPHA_VANTAGE_API_KEY").unwrap_or_default();
    let service = FinancialDataService::new(api_key).with_db(Arc::clone(&*db));
    service
        .fetch_news_sentiment(&symbol, lookback_hours)
        .await
        .map_err(|e| e.to_string())
}
