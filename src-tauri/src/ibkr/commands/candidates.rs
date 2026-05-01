//! Phase 4 — Tauri commands for the candidate-universe UI surface.
//!
//! Read-only over `candidate_universe` plus a manual "promote this row"
//! button so the user can move a staged candidate into the watchlist
//! without spinning up the agent. Mirrors `commands::sentiment` shape.

use std::sync::Arc;

use tauri::State;

use crate::services::candidate_promoter::{CandidatePromoter, PromotionOutcome};
use crate::services::candidate_scheduler::CandidateScheduler;
use crate::services::candidate_universe::types::{Candidate, CandidateFilter};
use crate::services::candidate_universe::CandidateUniverseService;
use crate::services::sentiment_surge_scanner::SentimentSurgeScanner;

#[derive(Debug, serde::Deserialize, Default)]
pub struct CandidatesQuery {
    pub source: Option<String>,
    pub min_score: Option<f64>,
    pub since_unix: Option<i64>,
    #[serde(default)]
    pub include_promoted: bool,
    pub limit: Option<usize>,
}

/// List staged candidates. Defaults match the agent inbox view —
/// promoted rows hidden, score-DESC ordered, capped at 100.
#[tauri::command]
pub async fn candidates_list(
    service: State<'_, Arc<CandidateUniverseService>>,
    query: Option<CandidatesQuery>,
) -> Result<Vec<Candidate>, String> {
    let q = query.unwrap_or_default();
    let filter = CandidateFilter {
        source_substring: q.source.filter(|s| !s.trim().is_empty()),
        min_score: q.min_score,
        since_last_seen: q.since_unix,
        include_promoted: q.include_promoted,
        limit: q.limit,
    };
    service.list(filter).await.map_err(|e| e.to_string())
}

/// Manually promote a staged candidate into the watchlist with
/// `source = "agent"` and the supplied `reason` as the row's notes.
/// Same path the `promote_candidate` MCP tool takes — both flow
/// through [`CandidatePromoter::promote_for_agent`] so the audit
/// stamp on the candidate row is consistent.
#[tauri::command]
pub async fn candidates_promote(
    promoter: State<'_, Arc<CandidatePromoter>>,
    symbol: String,
    reason: String,
) -> Result<bool, String> {
    let symbol_norm = symbol.trim().to_string();
    if symbol_norm.is_empty() {
        return Err("symbol must not be empty".into());
    }
    if reason.trim().is_empty() {
        return Err("reason must not be empty".into());
    }
    let outcome = promoter
        .promote_for_agent(&symbol_norm, &reason)
        .await
        .map_err(|e| e.to_string())?;
    Ok(matches!(outcome, PromotionOutcome::Promoted))
}

/// Force one scheduler tick — refreshes the sentiment-surge scan and
/// runs the decay sweep immediately. Useful from the UI when the user
/// wants a fresh pull without waiting on the cadence.
#[tauri::command]
pub async fn candidates_refresh_now(
    surge: State<'_, Arc<SentimentSurgeScanner>>,
    universe: State<'_, Arc<CandidateUniverseService>>,
) -> Result<RefreshOutcome, String> {
    let surge_outcome = surge.run_once().await.map_err(|e| e.to_string())?;
    let decay_outcome = universe.decay().await.map_err(|e| e.to_string())?;
    Ok(RefreshOutcome {
        surge_upserted: surge_outcome.upserted.len(),
        surge_auto_promoted: surge_outcome.auto_promoted.len(),
        decay_evicted: decay_outcome.evicted,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct RefreshOutcome {
    pub surge_upserted: usize,
    pub surge_auto_promoted: usize,
    pub decay_evicted: usize,
}

/// No-op handle confirming the scheduler is registered. Returns a
/// status string the UI can show alongside the "next refresh" badge.
#[tauri::command]
pub async fn candidates_scheduler_status(
    _scheduler: State<'_, Arc<CandidateScheduler>>,
) -> Result<&'static str, String> {
    Ok("registered")
}
