//! Phase 8 — Tauri commands backing the eval-harness UI.
//!
//! Read-only views over the calibration / cost-attribution / per-symbol
//! prediction history rolled up by the `eval_harness` service. Mirror
//! of the matching MCP read tools — same windowing, same shapes — so
//! the React eval dashboard and the agent see the same numbers.

use std::sync::Arc;

use chrono::Utc;
use tauri::State;

use crate::services::eval_harness::{
    self, CalibrationStats, CostAttribution, PredictionWithOutcome,
};
use crate::storage::Db;

const DEFAULT_WINDOW_DAYS: i64 = 30;
const MAX_WINDOW_DAYS: i64 = 365;
const HISTORY_DEFAULT_WINDOW_DAYS: i64 = 90;

#[tauri::command]
pub async fn eval_calibration_stats(
    db: State<'_, Arc<Db>>,
    window_days: Option<i64>,
) -> Result<CalibrationStats, String> {
    let window = window_days
        .unwrap_or(DEFAULT_WINDOW_DAYS)
        .clamp(1, MAX_WINDOW_DAYS);
    let since_unix = Utc::now().timestamp() - window * 86_400;
    eval_harness::calibration_stats(&db, window, since_unix)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn eval_cost_attribution(
    db: State<'_, Arc<Db>>,
    window_days: Option<i64>,
) -> Result<CostAttribution, String> {
    let window = window_days
        .unwrap_or(DEFAULT_WINDOW_DAYS)
        .clamp(1, MAX_WINDOW_DAYS);
    let since_unix = Utc::now().timestamp() - window * 86_400;
    eval_harness::cost_attribution(&db, window, since_unix)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn eval_prediction_history(
    db: State<'_, Arc<Db>>,
    symbol: String,
    window_days: Option<i64>,
) -> Result<Vec<PredictionWithOutcome>, String> {
    if symbol.trim().is_empty() {
        return Err("symbol must not be empty".to_string());
    }
    let window = window_days
        .unwrap_or(HISTORY_DEFAULT_WINDOW_DAYS)
        .clamp(1, MAX_WINDOW_DAYS);
    let since_unix = Utc::now().timestamp() - window * 86_400;
    eval_harness::prediction_history(&db, &symbol, since_unix)
        .await
        .map_err(|e| e.to_string())
}
