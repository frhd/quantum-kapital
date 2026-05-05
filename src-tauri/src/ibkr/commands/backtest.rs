//! Phase 6 — Tauri commands for the backtester.
//!
//! Thin wrappers over `Backtester::{run, get_run, list_runs, compare}`.
//! `backtest_run` blocks until the run finishes and returns the full
//! `BacktestResult` — the spec doc spoke of progress events, but for
//! v1 the runner is fast enough on the small fixture sizes the user
//! drives interactively that streaming progress would be over-design.
//! Heavy runs go through `qk-backtest` CLI, not this command.

use std::sync::Arc;

use tauri::State;

use crate::services::backtester::{
    BacktestComparison, BacktestResult, BacktestRunSummary, BacktestSpec, Backtester,
};

#[tauri::command]
pub async fn backtest_run(
    spec: BacktestSpec,
    bt: State<'_, Arc<Backtester>>,
) -> Result<BacktestResult, String> {
    bt.run(spec).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn backtest_get_run(
    run_id: String,
    bt: State<'_, Arc<Backtester>>,
) -> Result<Option<BacktestResult>, String> {
    bt.get_run(&run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn backtest_list_runs(
    limit: Option<u32>,
    bt: State<'_, Arc<Backtester>>,
) -> Result<Vec<BacktestRunSummary>, String> {
    bt.list_runs(limit.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn backtest_compare(
    run_id_a: String,
    run_id_b: String,
    bt: State<'_, Arc<Backtester>>,
) -> Result<Option<BacktestComparison>, String> {
    bt.compare(&run_id_a, &run_id_b)
        .await
        .map_err(|e| e.to_string())
}
