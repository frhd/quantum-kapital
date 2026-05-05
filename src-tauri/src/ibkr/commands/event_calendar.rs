//! Phase 5 — Tauri commands for the event-blackout gate.
//!
//! `event_calendar_lookup` returns next-earnings + days-to-FOMC for a
//! symbol so the SetupCard / pre-trade banner can render "earnings in 3
//! BD" copy without the frontend duplicating the gate logic.
//!
//! `event_calendar_force_refresh` discards the cache so a follow-up
//! lookup re-fetches from upstream (used before the morning sweep).
//!
//! `setup_override_blackout` lets the trader take a blackout-skipped
//! setup anyway. The override produces a fresh non-skipped `setups`
//! row, audited via `setup_blackout_overrides`, and re-fires
//! `SetupDetected` so the UI picks the new row up.

use std::sync::Arc;

use chrono::Utc;
use tauri::State;

use crate::events::AppEvent;
use crate::ibkr::state::IbkrState;
use crate::ibkr::types::tracker::Setup;
use crate::services::event_calendar::{EventCalendarLookup, EventCalendarService};
use crate::storage::Db;
use crate::strategies::SetupCandidate;

#[tauri::command]
pub async fn event_calendar_lookup(
    gate: State<'_, Arc<EventCalendarService>>,
    symbol: String,
) -> Result<EventCalendarLookup, String> {
    gate.lookup(&symbol, Utc::now())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn event_calendar_force_refresh(
    gate: State<'_, Arc<EventCalendarService>>,
) -> Result<(), String> {
    gate.force_refresh().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn setup_override_blackout(
    state: State<'_, IbkrState>,
    db: State<'_, Arc<Db>>,
    setup_id: i64,
    reason: String,
    actor: Option<String>,
) -> Result<Setup, String> {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        return Err("override reason must be non-empty".to_string());
    }
    // Pull the original skipped row.
    let original = state
        .tracker
        .get_setup(setup_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("setup#{setup_id} not found"))?;
    let skip_kind = match original.skipped_reason {
        Some(r) => r.as_str().to_string(),
        None => return Err(format!("setup#{setup_id} is not a skipped setup")),
    };

    // Reconstruct a SetupCandidate so insert_setup can land a fresh row
    // with the skip fields cleared. The strategy `&'static str` becomes
    // a leak — but the override path is rare and the leaked str pays
    // back the avoided allocation noise on the much-hotter detector
    // path. Mirrors the conviction_signal recovery in `risk_recompute_setup`.
    let strategy_static: &'static str = Box::leak(original.strategy.clone().into_boxed_str());
    let candidate = SetupCandidate {
        strategy: strategy_static,
        tag: crate::ibkr::types::StrategyTag::Custom(original.strategy.clone()),
        direction: original.direction,
        conviction_signal: 0.5, // sized fresh by the engine if wired
        trigger_price: original.trigger_price,
        stop_price: original.stop_price,
        targets: original.targets.clone(),
        raw_signals: original.raw_signals.clone(),
        timeframe: crate::ibkr::types::BarSize::Day1,
        detected_at: Utc::now(),
    };
    let new_setup = state
        .tracker
        .insert_setup(&original.symbol, &candidate)
        .await
        .map_err(|e| e.to_string())?;

    // Audit the override.
    let actor_str = actor.unwrap_or_else(|| "human".to_string());
    let new_id = new_setup.id;
    let original_id = original.id;
    let kind_for_db = skip_kind.clone();
    let reason_for_db = trimmed.to_string();
    let actor_for_db = actor_str.clone();
    let overridden_at = Utc::now().timestamp();
    let db_handle = db.inner().clone();
    db_handle
        .with_conn(move |conn| {
            conn.execute(
                "INSERT INTO setup_blackout_overrides \
                   (skipped_setup_id, new_setup_id, gate_kind, reason, actor, overridden_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    original_id,
                    new_id,
                    kind_for_db,
                    reason_for_db,
                    actor_for_db,
                    overridden_at,
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;

    let _ = state
        .event_emitter
        .emit(AppEvent::SetupDetected {
            setup: Box::new(new_setup.clone()),
            thesis: None,
        })
        .await;

    Ok(new_setup)
}

#[tauri::command]
pub async fn tracker_get_skipped_setups(
    state: State<'_, IbkrState>,
    since: Option<chrono::DateTime<Utc>>,
) -> Result<Vec<Setup>, String> {
    state
        .tracker
        .list_skipped_setups(since)
        .await
        .map_err(|e| e.to_string())
}
