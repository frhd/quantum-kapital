use chrono::{DateTime, Utc};
use std::sync::Arc;
use tauri::State;

use crate::ibkr::state::IbkrState;
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::news::NewsItem;
use crate::ibkr::types::tracker::{
    Setup, StrategyTag, TrackedTicker, TrackerSource, TrackerStatus,
};
use crate::services::eod_scheduler::EodScheduler;
use crate::services::financial_data_service::FinancialDataService;
use crate::services::historical_data_service::{HistoricalDataService, Lookback};
use crate::services::tracker_runner::{RunResult, TrackerRunner};
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

#[tauri::command]
pub async fn tracker_add(
    state: State<'_, IbkrState>,
    symbol: String,
    source: TrackerSource,
    source_meta: Option<serde_json::Value>,
    tags: Vec<StrategyTag>,
    notes: Option<String>,
) -> Result<TrackedTicker, String> {
    let row = state
        .tracker
        .add(&symbol, source, source_meta.clone(), tags, notes)
        .await
        .map_err(|e| e.to_string())?;

    // Phase 12: scanner-sourced rows promote straight to InPlay so the
    // intraday scheduler picks them up. Manual / news rows stay Watching
    // until a detector hit (or a manual flag) bumps them.
    if matches!(source, TrackerSource::Scanner) {
        if let Err(e) = state
            .state_machine
            .record_scanner_hit(&row.symbol, source_meta)
            .await
        {
            tracing::warn!("record_scanner_hit failed for {}: {e}", row.symbol);
        }
        // Re-read the row so the caller sees the post-promotion state.
        return state
            .tracker
            .get(&row.symbol)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "tracker row vanished after add".to_string());
    }
    Ok(row)
}

#[tauri::command]
pub async fn tracker_remove(state: State<'_, IbkrState>, symbol: String) -> Result<(), String> {
    state
        .tracker
        .remove(&symbol)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_list(
    state: State<'_, IbkrState>,
    status: Option<TrackerStatus>,
) -> Result<Vec<TrackedTicker>, String> {
    state.tracker.list(status).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_get(
    state: State<'_, IbkrState>,
    symbol: String,
) -> Result<Option<TrackedTicker>, String> {
    state.tracker.get(&symbol).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_set_tags(
    state: State<'_, IbkrState>,
    symbol: String,
    tags: Vec<StrategyTag>,
) -> Result<TrackedTicker, String> {
    state
        .tracker
        .set_tags(&symbol, tags)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_set_status(
    state: State<'_, IbkrState>,
    symbol: String,
    status: TrackerStatus,
    in_play_until: Option<DateTime<Utc>>,
    cool_down_until: Option<DateTime<Utc>>,
) -> Result<TrackedTicker, String> {
    state
        .tracker
        .set_status(&symbol, status, in_play_until, cool_down_until)
        .await
        .map_err(|e| e.to_string())
}

/// Phase 10 — gather fresh bars/news for one symbol (or every active
/// watchlist row when `symbol` is `None`), evaluate detectors, and
/// persist hits. Per-symbol failures are surfaced inside individual
/// `RunResult` entries and never short-circuit the batch.
#[tauri::command]
pub async fn tracker_run_now(
    runner: State<'_, Arc<TrackerRunner>>,
    symbol: Option<String>,
) -> Result<Vec<RunResult>, String> {
    match symbol {
        Some(s) => match runner.run_for(&s).await {
            Ok(setups) => Ok(vec![RunResult {
                symbol: s.to_uppercase(),
                setups,
                error: None,
            }]),
            Err(e) => Ok(vec![RunResult {
                symbol: s.to_uppercase(),
                setups: Vec::new(),
                error: Some(e.to_string()),
            }]),
        },
        None => runner.run_all().await.map_err(|e| e.to_string()),
    }
}

/// Phase 10 — read the persisted `setups` table. Both arguments are
/// optional: pass `symbol` to filter to one ticker, `since` (UTC) to
/// only return rows newer than the cutoff. Returns rows ordered by
/// `detected_at DESC`.
#[tauri::command]
pub async fn tracker_get_setups(
    state: State<'_, IbkrState>,
    symbol: Option<String>,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<Setup>, String> {
    state
        .tracker
        .list_setups(symbol.as_deref(), since)
        .await
        .map_err(|e| e.to_string())
}

/// Phase 13 — start the EOD scheduler. The Phase 14 intraday scheduler
/// will hang off the same command pair once it lands; for now this only
/// starts the EOD loop. Calling twice is safe — the second call replaces
/// the existing handle (mirrors the scanner stream pattern).
#[tauri::command]
pub async fn tracker_start_scheduler(
    state: State<'_, IbkrState>,
    scheduler: State<'_, Arc<EodScheduler>>,
) -> Result<(), String> {
    state.start_eod_scheduler(Arc::clone(&scheduler)).await
}

/// Phase 13 — stop the EOD scheduler if one is running. Idempotent.
#[tauri::command]
pub async fn tracker_stop_scheduler(state: State<'_, IbkrState>) -> Result<(), String> {
    state.stop_eod_scheduler().await;
    Ok(())
}
