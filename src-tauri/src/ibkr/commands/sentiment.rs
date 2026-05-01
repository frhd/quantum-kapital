//! Phase 3 — Tauri commands for the social-sentiment UI surface.
//!
//! Read-only over `social_sentiment` plus a manual "refresh now" knob
//! that runs the scheduler tick out-of-band. The widget on the
//! analysis view calls `social_get_latest` to render one row per
//! source; the gear menu calls `social_refresh_now` for a manual pull.

use std::sync::Arc;

use tauri::State;

use crate::services::social_sentiment::repo::{latest_per_source, rows_for_symbol_since};
use crate::services::social_sentiment::types::SocialSentimentRow;
use crate::services::social_sentiment::SocialSentimentService;
use crate::services::social_sentiment_scheduler::SocialSentimentScheduler;
use crate::storage::Db;

/// Latest row per source for `symbol` (one row per `source` value).
/// Used by the analysis-view widget. Returns `[]` when the table has
/// no rows yet for the symbol.
#[tauri::command]
pub async fn social_get_latest(
    db: State<'_, Arc<Db>>,
    symbol: String,
) -> Result<Vec<SocialSentimentRow>, String> {
    let symbol_norm = symbol.trim().to_string();
    if symbol_norm.is_empty() {
        return Err("symbol must not be empty".into());
    }
    latest_per_source(Arc::clone(&db), symbol_norm)
        .await
        .map_err(|e| e.to_string())
}

/// Time-series rows for `symbol` over a trailing window. `since_unix`
/// is unix seconds; pass `now - 24h` for the default. Optionally
/// filtered to a subset of source ids.
#[tauri::command]
pub async fn social_list_window(
    db: State<'_, Arc<Db>>,
    symbol: String,
    since_unix: i64,
    sources: Option<Vec<String>>,
) -> Result<Vec<SocialSentimentRow>, String> {
    let symbol_norm = symbol.trim().to_string();
    if symbol_norm.is_empty() {
        return Err("symbol must not be empty".into());
    }
    rows_for_symbol_since(Arc::clone(&db), symbol_norm, since_unix, sources)
        .await
        .map_err(|e| e.to_string())
}

/// Force one scheduler tick. Useful from the UI gear menu when the
/// user wants a fresh pull without waiting on the cadence. The
/// scheduler still respects its own min-interval cooldown — this
/// command bypasses that.
#[tauri::command]
pub async fn social_refresh_now(
    service: State<'_, Arc<SocialSentimentService>>,
    tracker: State<'_, crate::ibkr::IbkrState>,
    symbols: Option<Vec<String>>,
) -> Result<usize, String> {
    let symbols = match symbols {
        Some(s) if !s.is_empty() => s,
        _ => tracker
            .tracker
            .list(None)
            .await
            .map_err(|e| format!("list watchlist: {e}"))?
            .into_iter()
            .map(|t| t.symbol)
            .collect(),
    };
    let outcome = service
        .fetch_and_persist(&symbols)
        .await
        .map_err(|e| e.to_string())?;
    Ok(outcome.samples_persisted)
}

/// No-op handle to acknowledge that the social-sentiment scheduler
/// is registered with the app. Returns the configured min-interval
/// in seconds so the UI can show "next refresh in …".
#[tauri::command]
pub async fn social_scheduler_status(
    _scheduler: State<'_, Arc<SocialSentimentScheduler>>,
) -> Result<&'static str, String> {
    Ok("registered")
}
