use chrono::{DateTime, NaiveDate, Utc};
use std::sync::Arc;
use tauri::State;

use crate::ibkr::state::IbkrState;
use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::news::NewsItem;
use crate::ibkr::types::tracker::{
    Alert, AlertKind, Setup, StrategyTag, TrackedTicker, TrackerSource, TrackerStatus,
};
use crate::services::alerts::{list_alerts, mark_alerts_seen, ListAlertsQuery};
use crate::services::daily_ranker::{DailyRanker, MorningPack};
use crate::services::eod_scheduler::EodScheduler;
use crate::services::financial_data_service::FinancialDataService;
use crate::services::historical_data_service::{HistoricalDataService, Lookback};
use crate::services::intraday_scheduler::IntradayScheduler;
#[cfg(debug_assertions)]
use crate::services::llm_service::{LlmKind, LlmRequest, LlmService, Message, Role};
use crate::services::tracker_runner::{RunResult, TrackerRunner};

#[tauri::command]
pub async fn tracker_fetch_bars(
    service: State<'_, Arc<HistoricalDataService>>,
    symbol: String,
    bar_size: BarSize,
    lookback_days: u32,
) -> Result<Vec<HistoricalBar>, String> {
    service
        .fetch_bars(&symbol, bar_size, Lookback::Days(lookback_days))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_get_news(
    financial: State<'_, Arc<FinancialDataService>>,
    symbol: String,
    lookback_hours: u32,
) -> Result<Vec<NewsItem>, String> {
    financial
        .fetch_news_sentiment(&symbol, lookback_hours)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_add(
    state: State<'_, IbkrState>,
    symbol: String,
    source: TrackerSource,
    source_meta: Option<serde_json::Value>,
    tags: Vec<StrategyTag>,
    notes: Option<String>,
) -> Result<TrackedTicker, String> {
    let row = state
        .tracker
        .add(&symbol, source, source_meta.clone(), tags, notes)
        .await
        .map_err(|e| e.to_string())?;

    // Phase 12: scanner-sourced rows promote straight to InPlay so the
    // intraday scheduler picks them up. Manual / news rows stay Watching
    // until a detector hit (or a manual flag) bumps them.
    if matches!(source, TrackerSource::Scanner) {
        if let Err(e) = state
            .state_machine
            .record_scanner_hit(&row.symbol, source_meta)
            .await
        {
            tracing::warn!("record_scanner_hit failed for {}: {e}", row.symbol);
        }
        // Re-read the row so the caller sees the post-promotion state.
        return state
            .tracker
            .get(&row.symbol)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "tracker row vanished after add".to_string());
    }
    Ok(row)
}

#[tauri::command]
pub async fn tracker_remove(state: State<'_, IbkrState>, symbol: String) -> Result<(), String> {
    state
        .tracker
        .remove(&symbol)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_list(
    state: State<'_, IbkrState>,
    status: Option<TrackerStatus>,
) -> Result<Vec<TrackedTicker>, String> {
    state.tracker.list(status).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_get(
    state: State<'_, IbkrState>,
    symbol: String,
) -> Result<Option<TrackedTicker>, String> {
    state.tracker.get(&symbol).await.map_err(|e| e.to_string())
}

/// Soft-archive a tracked ticker (and every setup beneath it). Archived
/// rows drop out of `tracker_list`, the detector pipeline, the state
/// machine, and alert emission without losing history. Returns
/// `NotFound` only when the symbol has never been tracked at all.
#[tauri::command]
pub async fn tracker_archive(state: State<'_, IbkrState>, symbol: String) -> Result<(), String> {
    state
        .tracker
        .archive_ticker(&symbol)
        .await
        .map_err(|e| e.to_string())
}

/// Inverse of `tracker_archive`. Restores the ticker and its setups to
/// active reads. Returns `NotFound` only when the symbol has never been
/// tracked at all.
#[tauri::command]
pub async fn tracker_unarchive(state: State<'_, IbkrState>, symbol: String) -> Result<(), String> {
    state
        .tracker
        .unarchive_ticker(&symbol)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_set_tags(
    state: State<'_, IbkrState>,
    symbol: String,
    tags: Vec<StrategyTag>,
) -> Result<TrackedTicker, String> {
    state
        .tracker
        .set_tags(&symbol, tags)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tracker_set_status(
    state: State<'_, IbkrState>,
    symbol: String,
    status: TrackerStatus,
    in_play_until: Option<DateTime<Utc>>,
    cool_down_until: Option<DateTime<Utc>>,
) -> Result<TrackedTicker, String> {
    state
        .tracker
        .set_status(&symbol, status, in_play_until, cool_down_until)
        .await
        .map_err(|e| e.to_string())
}

/// Phase 10 — gather fresh bars/news for one symbol (or every active
/// watchlist row when `symbol` is `None`), evaluate detectors, and
/// persist hits. Per-symbol failures are surfaced inside individual
/// `RunResult` entries and never short-circuit the batch.
#[tauri::command]
pub async fn tracker_run_now(
    runner: State<'_, Arc<TrackerRunner>>,
    symbol: Option<String>,
) -> Result<Vec<RunResult>, String> {
    match symbol {
        Some(s) => match runner.run_for(&s).await {
            Ok(setups) => Ok(vec![RunResult {
                symbol: s.to_uppercase(),
                setups,
                error: None,
            }]),
            Err(e) => Ok(vec![RunResult {
                symbol: s.to_uppercase(),
                setups: Vec::new(),
                error: Some(e.to_string()),
            }]),
        },
        None => runner.run_all().await.map_err(|e| e.to_string()),
    }
}

/// Phase 10 — read the persisted `setups` table. Both arguments are
/// optional: pass `symbol` to filter to one ticker, `since` (UTC) to
/// only return rows newer than the cutoff. Returns rows ordered by
/// `detected_at DESC`.
#[tauri::command]
pub async fn tracker_get_setups(
    state: State<'_, IbkrState>,
    symbol: Option<String>,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<Setup>, String> {
    state
        .tracker
        .list_setups(symbol.as_deref(), since)
        .await
        .map_err(|e| e.to_string())
}

/// Phase 13/14 — start both the EOD sweep and the intraday RTH
/// scheduler. Calling twice is safe — each `start_*` call replaces
/// any existing handle (mirrors the scanner stream pattern).
#[tauri::command]
pub async fn tracker_start_scheduler(
    state: State<'_, IbkrState>,
    eod_scheduler: State<'_, Arc<EodScheduler>>,
    intraday_scheduler: State<'_, Arc<IntradayScheduler>>,
) -> Result<(), String> {
    state
        .start_eod_scheduler(Arc::clone(&eod_scheduler))
        .await?;
    state
        .start_intraday_scheduler(Arc::clone(&intraday_scheduler))
        .await
}

/// Phase 13/14 — stop both schedulers if they are running. Idempotent.
#[tauri::command]
pub async fn tracker_stop_scheduler(state: State<'_, IbkrState>) -> Result<(), String> {
    state.stop_eod_scheduler().await;
    state.stop_intraday_scheduler().await;
    Ok(())
}

/// Phase 20 — fetch the persisted morning pack for `date` (or the
/// most recent one when `date` is `None`). Returns `None` when no pack
/// has ever been generated.
#[tauri::command]
pub async fn tracker_get_morning_pack(
    ranker: State<'_, Arc<DailyRanker>>,
    date: Option<NaiveDate>,
) -> Result<Option<MorningPack>, String> {
    match date {
        Some(d) => ranker.get_pack(d).await.map_err(|e| e.to_string()),
        None => ranker.get_latest().await.map_err(|e| e.to_string()),
    }
}

/// Phase 21 — read a slice of the alert feed. All filters are
/// AND-combined; rows come back newest-first. The frontend consumes the
/// raw `Alert` rows; `payload.symbol` lets a click route to the analysis
/// view.
#[tauri::command]
pub async fn tracker_list_alerts(
    db: State<'_, Arc<crate::storage::Db>>,
    limit: Option<u32>,
    offset: Option<u32>,
    since: Option<DateTime<Utc>>,
    kind: Option<AlertKind>,
    only_unseen: Option<bool>,
) -> Result<Vec<Alert>, String> {
    let q = ListAlertsQuery {
        limit: limit.unwrap_or(50),
        offset: offset.unwrap_or(0),
        since,
        kind,
        only_unseen: only_unseen.unwrap_or(false),
        unenriched_only: false,
    };
    list_alerts(&db, q).await.map_err(|e| e.to_string())
}

/// Phase 21 — mark every alert id in `ids` as seen. Returns the number
/// of rows actually flipped (already-seen and unknown ids contribute 0).
#[tauri::command]
pub async fn tracker_mark_alerts_seen(
    db: State<'_, Arc<crate::storage::Db>>,
    ids: Vec<i64>,
) -> Result<usize, String> {
    mark_alerts_seen(&db, ids).await.map_err(|e| e.to_string())
}

/// Phase 16 — debug-only Anthropic smoke test. Sends a tiny prompt to
/// Sonnet 4.6 and returns the assistant's reply, which lets a developer
/// confirm an `ANTHROPIC_API_KEY` is wired correctly and that a row
/// lands in the `llm_calls` ledger. Compiled out of release builds.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn tracker_llm_smoke_test(llm: State<'_, Arc<LlmService>>) -> Result<String, String> {
    let req = LlmRequest {
        kind: LlmKind::Thesis,
        model: "claude-sonnet-4-6",
        max_tokens: 64,
        system: Vec::new(),
        messages: vec![Message {
            role: Role::User,
            content: "Reply with the single word: pong".to_string(),
        }],
        tools: None,
        tool_choice: None,
        setup_id: None,
        loop_name: None,
    };
    let resp = llm.message(req).await.map_err(|e| e.to_string())?;
    Ok(resp.text.unwrap_or_default())
}
