//! Quant-decisions Phase 1 — Tauri commands for the risk engine.
//!
//! `risk_get_config` / `risk_set_config` round-trip the live
//! `RiskConfig`. `risk_recompute_setup` re-runs sizing for an
//! existing row (used after the trader force-refreshes the
//! equity snapshot).

use std::sync::Arc;

use tauri::State;

use crate::config::SettingsState;
use crate::events::AppEvent;
use crate::ibkr::state::IbkrState;
use crate::services::risk_engine::{EquitySnapshot, RiskConfig, RiskEngine, Sizing};
use crate::strategies::SetupCandidate;

#[tauri::command]
pub async fn risk_get_config(engine: State<'_, Arc<RiskEngine>>) -> Result<RiskConfig, String> {
    Ok(engine.config().await)
}

#[tauri::command]
pub async fn risk_set_config(
    cfg: RiskConfig,
    engine: State<'_, Arc<RiskEngine>>,
    settings: State<'_, SettingsState>,
) -> Result<(), String> {
    // Mirror update_settings behaviour: persist to settings.json so a
    // restart picks up the new knobs, then push to the live engine.
    let snapshot = {
        let mut guard = settings.config.write().await;
        guard.risk_engine = cfg.clone();
        guard.clone()
    };
    snapshot
        .save()
        .await
        .map_err(|e| format!("save settings.json: {e}"))?;
    engine.set_config(cfg).await;
    Ok(())
}

/// Re-run sizing for `setup_id`. Used after `risk_set_config` to
/// pick up new knobs, or after `risk_refresh_equity` (force a
/// fresh NLV pull). Re-emits `SetupSized` so the UI refreshes.
#[tauri::command]
pub async fn risk_recompute_setup(
    setup_id: i64,
    engine: State<'_, Arc<RiskEngine>>,
    state: State<'_, IbkrState>,
) -> Result<Sizing, String> {
    let setup = state
        .tracker
        .get_setup(setup_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("setup#{setup_id} not found"))?;

    let candidate = SetupCandidate {
        // The strategy field is borrowed `&'static str` on the
        // candidate; recompute only needs an opaque marker since
        // the original detector name isn't load-bearing for sizing
        // math.
        strategy: "recompute",
        tag: crate::ibkr::types::StrategyTag::Custom(setup.strategy.clone()),
        direction: setup.direction,
        conviction_signal: conviction_signal_from_setup(&setup),
        trigger_price: setup.trigger_price,
        stop_price: setup.stop_price,
        targets: setup.targets.clone(),
        raw_signals: setup.raw_signals.clone(),
        timeframe: crate::ibkr::types::BarSize::Day1,
        detected_at: setup.detected_at,
    };

    let (sizing, _snap) = engine
        .size_for_candidate(&candidate)
        .await
        .map_err(|e| e.to_string())?;
    let refreshed = state
        .tracker
        .update_setup_sizing(setup_id, &sizing)
        .await
        .map_err(|e| e.to_string())?;
    let _ = state
        .event_emitter
        .emit(AppEvent::SetupSized {
            setup_id: refreshed.id,
            symbol: refreshed.symbol.clone(),
            sizing: sizing.clone(),
        })
        .await;
    Ok(sizing)
}

/// Force-fetch equity from IBKR and overwrite today's snapshot.
/// Returned for convenience; the next `risk_recompute_setup`
/// call against the same account picks up the new NLV.
#[tauri::command]
pub async fn risk_refresh_equity(
    engine: State<'_, Arc<RiskEngine>>,
) -> Result<EquitySnapshot, String> {
    engine.refresh_equity().await.map_err(|e| e.to_string())
}

/// Recover the conviction signal from a stored `Setup`. Pre-P1 rows
/// have no `conviction_grade` persisted — we map back from the
/// stored grade (A/B/C) to a representative signal, falling through
/// to the C floor when the grade column is absent.
fn conviction_signal_from_setup(setup: &crate::ibkr::types::tracker::Setup) -> f64 {
    use crate::services::risk_engine::ConvictionGrade;
    let grade = setup
        .sizing
        .as_ref()
        .map(|s| s.conviction_grade)
        .unwrap_or(ConvictionGrade::C);
    match grade {
        ConvictionGrade::A => 0.85,
        ConvictionGrade::B => 0.6,
        ConvictionGrade::C => 0.3,
    }
}
