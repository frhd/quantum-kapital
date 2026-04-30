use std::sync::Arc;

use chrono::Utc;
use tauri::State;

use crate::config::settings::AutoScannerConfig;
use crate::ibkr::state::IbkrState;
use crate::services::auto_scanner::{AutoScannerScheduler, AutoScannerService};

/// Start the polling loop. Idempotent — calling twice replaces the
/// existing handle (mirrors `tracker_start_scheduler`).
#[tauri::command]
pub async fn auto_scanner_start(
    state: State<'_, IbkrState>,
    scheduler: State<'_, Arc<AutoScannerScheduler>>,
) -> Result<(), String> {
    state.start_auto_scanner(Arc::clone(&scheduler)).await
}

/// Stop the polling loop if it's running. No-op otherwise.
#[tauri::command]
pub async fn auto_scanner_stop(state: State<'_, IbkrState>) -> Result<(), String> {
    state.stop_auto_scanner().await;
    Ok(())
}

/// Return the current config — useful for the UI to render the
/// "what scans will run" status panel.
#[tauri::command]
pub async fn auto_scanner_get_config(
    service: State<'_, Arc<AutoScannerService>>,
) -> Result<AutoScannerConfig, String> {
    Ok(service.config().await)
}

/// Update the config in-memory (does not persist to disk — the
/// settings panel is responsible for that). Picks up on the next tick.
#[tauri::command]
pub async fn auto_scanner_set_config(
    service: State<'_, Arc<AutoScannerService>>,
    config: AutoScannerConfig,
) -> Result<(), String> {
    service.set_config(config).await;
    Ok(())
}

/// Manual trigger that bypasses the cadence cursor. Useful during
/// development and for a "Run scan now" UI button. Returns a brief
/// summary of what got promoted.
#[tauri::command]
pub async fn auto_scanner_run_once(
    service: State<'_, Arc<AutoScannerService>>,
) -> Result<RunSummaryDto, String> {
    let summary = service.run_once(Utc::now()).await?;
    Ok(RunSummaryDto {
        added: summary.added,
        skipped: summary.skipped,
        errors: summary.errors,
    })
}

#[derive(serde::Serialize)]
pub struct RunSummaryDto {
    pub added: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}
