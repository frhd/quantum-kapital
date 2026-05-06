//! Phase 8 — Tauri commands for the portfolio-risk dashboard +
//! concentration gate.
//!
//! Six commands round-trip the live state to the frontend:
//!
//!   - `portfolio_risk_snapshot` — current `PortfolioRisk` view.
//!   - `portfolio_risk_history`  — time-series of persisted snapshots.
//!   - `concentration_get_config` / `concentration_set_config` —
//!     read / write the live limits.
//!   - `concentration_check`     — pre-trade hypothetical: "if I add
//!     this candidate, what does the gate say?". Used by the
//!     SetupCard's TakeSetupModal banner.
//!   - `concentration_record_override` — audit row written when the
//!     trader overrides a `block` or proceeds past a `warn`.

use std::sync::Arc;

use tauri::State;

use crate::config::SettingsState;
use crate::services::portfolio_risk::{
    ConcentrationConfig, GateInput, GateResult, PortfolioRiskService, PortfolioSnapshotRow,
    PortfolioRisk,
};

#[tauri::command]
pub async fn portfolio_risk_snapshot(
    svc: State<'_, Arc<PortfolioRiskService>>,
) -> Result<PortfolioRisk, String> {
    svc.snapshot().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn portfolio_risk_history(
    svc: State<'_, Arc<PortfolioRiskService>>,
    limit: Option<u32>,
) -> Result<Vec<PortfolioSnapshotRow>, String> {
    svc.history(limit.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn concentration_get_config(
    svc: State<'_, Arc<PortfolioRiskService>>,
) -> Result<ConcentrationConfig, String> {
    Ok(svc.config().await)
}

#[tauri::command]
pub async fn concentration_set_config(
    cfg: ConcentrationConfig,
    svc: State<'_, Arc<PortfolioRiskService>>,
    settings: State<'_, SettingsState>,
) -> Result<(), String> {
    // Mirror the risk-engine pattern: persist to settings.json so a
    // restart picks up the new knobs, then push to the live service.
    let snapshot = {
        let mut guard = settings.config.write().await;
        guard.concentration = cfg.clone();
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
pub struct ConcentrationCheckInput {
    pub symbol: String,
    pub projected_dollar_risk_cents: i64,
    pub strategy: String,
    #[serde(default)]
    pub momentum_bucket: Option<String>,
}

#[tauri::command]
pub async fn concentration_check(
    input: ConcentrationCheckInput,
    svc: State<'_, Arc<PortfolioRiskService>>,
) -> Result<GateResult, String> {
    let gate = svc.gate().await.map_err(|e| e.to_string())?;
    let momentum = input.momentum_bucket.as_deref();
    let strategy_static: &'static str = Box::leak(input.strategy.into_boxed_str());
    let symbol_static: &'static str = Box::leak(input.symbol.into_boxed_str());
    let momentum_static: Option<&'static str> = momentum
        .map(|s| -> &'static str { Box::leak(s.to_string().into_boxed_str()) });
    let gi = GateInput {
        symbol: symbol_static,
        projected_dollar_risk_cents: input.projected_dollar_risk_cents,
        strategy: strategy_static,
        momentum_bucket: momentum_static,
    };
    Ok(gate.check(&gi).await)
}

#[derive(serde::Deserialize)]
pub struct OverrideInput {
    pub setup_id: i64,
    pub gate_kind: String,
    pub reason: String,
    #[serde(default)]
    pub actor: Option<String>,
}

#[tauri::command]
pub async fn concentration_record_override(
    input: OverrideInput,
    svc: State<'_, Arc<PortfolioRiskService>>,
) -> Result<i64, String> {
    let trimmed = input.reason.trim();
    if trimmed.is_empty() {
        return Err("override reason must be non-empty".to_string());
    }
    let actor = input.actor.unwrap_or_else(|| "human".to_string());
    svc.record_override(input.setup_id, &input.gate_kind, trimmed, &actor)
        .await
        .map_err(|e| e.to_string())
}
