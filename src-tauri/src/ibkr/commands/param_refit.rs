//! Phase 10 — Tauri commands for the param-refit eval surface.
//!
//! Four commands round-trip the live state to the frontend:
//!
//!   - `param_refit_run_now`     — manual trigger of the monthly sweep.
//!   - `param_refit_history`     — per-detector vintage timeline.
//!   - `param_refit_get_active`  — snapshot of every detector's
//!     currently-locked vintage.
//!   - `param_refit_lock_manual` — admin path to lock a manual params
//!     override without running a sweep.

use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::services::param_refit::{LockSource, ParamRefitService, ParamVintage, RefitReport};

#[derive(Debug, Deserialize)]
pub struct RunNowInput {
    /// Optional single-detector filter (e.g. `"breakout"`). When
    /// `None`, the sweep runs for every detector.
    #[serde(default)]
    pub detector: Option<String>,
}

#[tauri::command]
pub async fn param_refit_run_now(
    input: Option<RunNowInput>,
    svc: State<'_, Arc<ParamRefitService>>,
) -> Result<RefitReport, String> {
    let input = input.unwrap_or(RunNowInput { detector: None });
    match input.detector {
        Some(d) => {
            let outcome = svc
                .run_for_detector(&d, LockSource::Manual)
                .await
                .map_err(|e| e.to_string())?;
            Ok(RefitReport {
                refit_at: chrono::Utc::now(),
                source: LockSource::Manual.as_str().to_string(),
                outcomes: vec![outcome],
            })
        }
        None => svc
            .run_monthly(LockSource::Manual)
            .await
            .map_err(|e| e.to_string()),
    }
}

#[tauri::command]
pub async fn param_refit_history(
    detector: String,
    limit: Option<u32>,
    svc: State<'_, Arc<ParamRefitService>>,
) -> Result<Vec<ParamVintage>, String> {
    svc.history_for(&detector, limit.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn param_refit_get_active(
    svc: State<'_, Arc<ParamRefitService>>,
) -> Result<Vec<ParamVintage>, String> {
    svc.active_all().await.map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct LockManualInput {
    pub detector: String,
    pub params_json: serde_json::Value,
    /// Operator's estimate of the locked vintage's objective. The
    /// next refit's lock-on-improvement check uses this as the
    /// baseline; pass `0.0` to make any successful refit unseat it.
    pub objective_value: f64,
    /// Number of OOS trades used to derive the estimate. Recorded
    /// for the audit trail; the constraint guard isn't applied to
    /// manual locks.
    pub oos_n_trades: i64,
    #[serde(default)]
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn param_refit_lock_manual(
    input: LockManualInput,
    svc: State<'_, Arc<ParamRefitService>>,
) -> Result<ParamVintage, String> {
    let trimmed_notes = input.notes.as_ref().map(|s| s.trim().to_string());
    svc.lock_manual(
        &input.detector,
        input.params_json,
        input.objective_value,
        input.oos_n_trades,
        trimmed_notes,
    )
    .await
    .map_err(|e| e.to_string())
}
