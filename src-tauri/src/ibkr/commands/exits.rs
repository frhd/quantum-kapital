//! Phase 7 — Tauri commands for the exit-policy + bracket-reviser
//! surface.
//!
//! `exits_get_policy` returns the policy version + sample plan for a
//! given strategy + ATR; the modal uses it to render the ladder
//! preview before the trader confirms.
//!
//! `bracket_reviser_status` returns the live trail snapshot for every
//! open bracket — what the panel renders for the "trail at $X, last
//! step at HH:MM" surface.
//!
//! `bracket_revert_to_static` is the panic-button: cancel the
//! existing bracket via `OrderTicket::cancel` and let the trader
//! re-place under the legacy static policy. Avoids hand-modifying the
//! IBKR-side OCA group.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::services::bracket_reviser::{BracketReviser, BracketReviserSnapshot};
use crate::services::order_ticket::{BracketGroupRecord, OrderTicket};
use crate::strategies::exits::{ExitPlan, ExitPolicyContext, ExitPolicyRegistry, V2_ATR_SCALED};
use crate::strategies::Direction;

/// Shape returned by `exits_get_policy`. Mirrors what the modal
/// renders in `ExitPlanCard` — policy version, targets, trail spec
/// and time-stop horizon, all derived from a sample (trigger, stop,
/// ATR) tuple the caller picks.
#[derive(Debug, Serialize)]
pub struct ExitPolicyPreview {
    pub policy_version: String,
    pub plan: Option<ExitPlan>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn exits_get_policy(
    strategy: String,
    direction: String,
    trigger_price: f64,
    stop_price: f64,
    atr: Option<f64>,
) -> Result<ExitPolicyPreview, String> {
    let registry = ExitPolicyRegistry::default_for_phase_7();
    let policy = registry.for_strategy(&strategy);
    let dir = match direction.as_str() {
        "long" => Direction::Long,
        "short" => Direction::Short,
        other => return Err(format!("invalid direction '{other}' (expected long|short)")),
    };
    let ctx = ExitPolicyContext {
        direction: dir,
        trigger_price,
        stop_price,
        atr,
        strategy: Box::leak(strategy.clone().into_boxed_str()) as &'static str,
    };
    let version = policy.version().to_string();
    match policy.build_plan(&ctx) {
        Ok(plan) => Ok(ExitPolicyPreview {
            policy_version: version,
            plan: Some(plan),
            error: None,
        }),
        Err(e) => Ok(ExitPolicyPreview {
            policy_version: version,
            plan: None,
            error: Some(e.to_string()),
        }),
    }
}

#[tauri::command]
pub async fn bracket_reviser_status(
    reviser: State<'_, Arc<BracketReviser>>,
) -> Result<Vec<BracketReviserSnapshot>, String> {
    reviser.snapshot().await.map_err(|e| e.to_string())
}

/// Cancel a bracket that's running under the v2 ATR-scaled policy
/// and let the trader re-place it under v1_static. The cancel goes
/// through `OrderTicket::cancel` so the existing audit trail
/// (`BracketStatusChanged` event, `bracket_groups.last_status`
/// flips) stays intact. Re-placement is a separate user-initiated
/// `order_ticket_take_setup` call — this command does NOT auto-
/// re-place, by design (every parent order is human-initiated).
#[tauri::command]
pub async fn bracket_revert_to_static(
    parent_order_id: i32,
    ticket: State<'_, Arc<OrderTicket>>,
) -> Result<BracketGroupRecord, String> {
    ticket
        .cancel(parent_order_id)
        .await
        .map_err(|e| e.to_string())
}

/// Stub for `exits_set_policy`. Master phase doc reserves an admin
/// override path for switching a detector's policy, but Phase 7
/// ships with the registry hardcoded to `default_for_phase_7`. A
/// real settings-driven override lands when the operator surfaces
/// the comparator results from the 4-week shadow run; until then,
/// this command returns an error so callers know the path is not
/// load-bearing yet.
#[tauri::command]
pub async fn exits_set_policy(strategy: String, version: String) -> Result<(), String> {
    if version != crate::strategies::exits::V1_STATIC && version != V2_ATR_SCALED {
        return Err(format!(
            "unknown policy version '{version}' (expected v1_static or v2_atr_scaled)"
        ));
    }
    Err(format!(
        "exits_set_policy: per-strategy override unsupported in P7 — \
         strategy={strategy} requested={version}. Re-open when shadow-mode \
         comparison surfaces a settings.json knob."
    ))
}
