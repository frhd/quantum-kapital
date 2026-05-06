//! Phase 11 — Tauri commands for the tilt circuit breaker.
//!
//! Three commands round-trip the live tilt state to the frontend:
//!
//!   - `tilt_guard_status`   — read-only snapshot. Cheap; called on
//!     mount + on every `tilt-activated` / `tilt-released` event.
//!   - `tilt_guard_override` — close the open episode with a logged
//!     reason. Mirrored to `gate_overrides`.
//!   - `tilt_guard_history`  — past episodes for the trader-profile
//!     rollup card.
//!
//! No "force activate" command — activation is policy, not user input.

use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Deserialize;
use tauri::State;

use crate::services::tilt_guard::{TiltEpisodeView, TiltGuardService, TiltStatus};

#[tauri::command]
pub async fn tilt_guard_status(
    svc: State<'_, Arc<TiltGuardService>>,
) -> Result<TiltStatus, String> {
    let account = svc.current_account().await.map_err(|e| e.to_string())?;
    svc.evaluate(&account).await.map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct TiltOverrideInput {
    pub reason: String,
}

#[tauri::command]
pub async fn tilt_guard_override(
    input: TiltOverrideInput,
    svc: State<'_, Arc<TiltGuardService>>,
) -> Result<TiltStatus, String> {
    let trimmed = input.reason.trim();
    if trimmed.is_empty() {
        return Err("override reason must be non-empty".to_string());
    }
    let account = svc.current_account().await.map_err(|e| e.to_string())?;
    svc.override_pause(&account, trimmed.to_string())
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
pub struct TiltHistoryInput {
    /// Days of history to walk back. Default: 30.
    #[serde(default)]
    pub days: Option<u32>,
}

#[tauri::command]
pub async fn tilt_guard_history(
    input: Option<TiltHistoryInput>,
    svc: State<'_, Arc<TiltGuardService>>,
) -> Result<Vec<TiltEpisodeView>, String> {
    let days = input.and_then(|i| i.days).unwrap_or(30) as i64;
    let since: DateTime<Utc> = Utc::now() - ChronoDuration::days(days);
    let account = svc.current_account().await.map_err(|e| e.to_string())?;
    svc.history(&account, since)
        .await
        .map_err(|e| e.to_string())
}
