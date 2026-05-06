//! Phase 9 — Tauri commands for the regime indicator + override audit.
//!
//! Five commands round-trip the live state to the frontend:
//!
//!   - `regime_current`            — cached classification + inputs.
//!   - `regime_history`            — newest-first snapshot list.
//!   - `regime_force_recompute`    — operator-driven recompute.
//!   - `regime_get_config` /
//!     `regime_set_config`         — read / write the live filter map.
//!   - `regime_record_override`    — audit row for an override take.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::config::SettingsState;
use crate::services::regime::{
    Regime, RegimeConfig, RegimeService, RegimeSnapshotRow, SnapshotSource,
};

/// Frontend-facing shape for the cached snapshot. Flat so the React
/// pill component can render fields directly.
#[derive(Debug, Clone, Serialize)]
pub struct RegimeCurrentResponse {
    pub snapshot_id: i64,
    pub at_unix: i64,
    pub raw: Regime,
    pub stable: Regime,
    pub source: String,
    pub inputs_summary: serde_json::Value,
    pub missing: Vec<String>,
}

#[tauri::command]
pub async fn regime_current(
    svc: State<'_, Arc<RegimeService>>,
) -> Result<RegimeCurrentResponse, String> {
    let cached = svc.current().await.map_err(|e| e.to_string())?;
    Ok(RegimeCurrentResponse {
        snapshot_id: cached.snapshot_id,
        at_unix: cached.at.timestamp(),
        raw: cached.raw,
        stable: cached.stable,
        source: cached.source.as_str().to_string(),
        inputs_summary: serde_json::to_value(&cached.inputs).unwrap_or(serde_json::Value::Null),
        missing: cached.inputs.missing.clone(),
    })
}

#[tauri::command]
pub async fn regime_history(
    svc: State<'_, Arc<RegimeService>>,
    limit: Option<u32>,
) -> Result<Vec<RegimeSnapshotRow>, String> {
    svc.history(limit.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn regime_force_recompute(
    svc: State<'_, Arc<RegimeService>>,
) -> Result<RegimeCurrentResponse, String> {
    let cached = svc
        .snapshot(SnapshotSource::ForceRecompute)
        .await
        .map_err(|e| e.to_string())?;
    Ok(RegimeCurrentResponse {
        snapshot_id: cached.snapshot_id,
        at_unix: cached.at.timestamp(),
        raw: cached.raw,
        stable: cached.stable,
        source: cached.source.as_str().to_string(),
        inputs_summary: serde_json::to_value(&cached.inputs).unwrap_or(serde_json::Value::Null),
        missing: cached.inputs.missing.clone(),
    })
}

#[tauri::command]
pub async fn regime_get_config(svc: State<'_, Arc<RegimeService>>) -> Result<RegimeConfig, String> {
    Ok(svc.config().await)
}

#[tauri::command]
pub async fn regime_set_config(
    cfg: RegimeConfig,
    svc: State<'_, Arc<RegimeService>>,
    settings: State<'_, SettingsState>,
) -> Result<(), String> {
    let snapshot = {
        let mut guard = settings.config.write().await;
        guard.regime = cfg.clone();
        guard.clone()
    };
    snapshot
        .save()
        .await
        .map_err(|e| format!("save settings.json: {e}"))?;
    svc.set_config(cfg).await;
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct RegimeOverrideInput {
    pub setup_id: i64,
    pub reason: String,
    #[serde(default)]
    pub actor: Option<String>,
}

#[tauri::command]
pub async fn regime_record_override(
    input: RegimeOverrideInput,
    svc: State<'_, Arc<RegimeService>>,
) -> Result<i64, String> {
    let trimmed = input.reason.trim();
    if trimmed.is_empty() {
        return Err("override reason must be non-empty".to_string());
    }
    let actor = input.actor.unwrap_or_else(|| "human".to_string());
    svc.record_override(input.setup_id, trimmed, &actor)
        .await
        .map_err(|e| e.to_string())
}
