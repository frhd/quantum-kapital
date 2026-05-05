//! Phase 6 — `services/backtester/`: bar-replay harness for the live
//! detector registry.
//!
//! Public entry point is [`Backtester::run`]: takes a [`BacktestSpec`],
//! replays each symbol's daily bars through the registered detectors,
//! sizes via P1 risk-engine, fills via [`fill_model::FillModel`], and
//! aggregates per-trade outcomes into a [`BacktestResult`].
//!
//! Determinism contract: same spec ⇒ same `spec_hash` ⇒ same trade
//! sequence ⇒ same headline metrics. The orchestrator persists both
//! the result JSON and the per-trade rows to `backtest_runs` /
//! `backtest_trades` (V22 migration). Re-running an already-stored
//! spec is allowed; each call gets a fresh `run_id`.

#![allow(dead_code)]

pub mod bars_reader;
pub mod fill_model;
pub mod replay;
pub mod results;
pub mod spec;
pub mod walk_forward;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use chrono::Utc;
use rusqlite::params;
use thiserror::Error;
use tracing::{info, warn};

use crate::services::event_calendar::EventCalendarService;
use crate::services::tca::AttributionService;
use crate::storage::{Db, StorageError};
use crate::strategies::{DetectorRegistry, DetectorsConfig};

// Public re-exports — surfaces the types the Tauri commands, the
// `qk-backtest` CLI, and the lib's downstream consumers need.
#[allow(unused_imports)]
pub use bars_reader::{BarsReader, DbBarsReader};
#[allow(unused_imports)]
pub use fill_model::{
    apply_slippage, build_fill_model, CalibratedFillModel, FillModel, FillSide, NaiveNextOpenFill,
};
#[allow(unused_imports)]
pub use results::{
    aggregate, AggregateDiagnostics, BacktestResult, BacktestTrade, ExitReason, MonthRollup,
    StrategyRollup,
};
#[allow(unused_imports)]
pub use spec::{
    BacktestSpec, FillModelKind, PositionSizingMode, SpecValidationError, WalkForwardSplits,
    MAX_HISTORY_DAYS, MAX_SYMBOLS_PER_RUN,
};
#[allow(unused_imports)]
pub use walk_forward::{build_splits, filter_oos, Split};

#[derive(Debug, Error)]
pub enum BacktesterError {
    #[error("invalid spec: {0}")]
    InvalidSpec(#[from] SpecValidationError),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("calibration source unavailable: {0}")]
    Calibration(String),
}

pub type Result<T> = std::result::Result<T, BacktesterError>;

/// Service handle. Keeps `Arc` clones of dependencies so each `run`
/// gets a fresh `Backtester` view without re-acquiring services.
pub struct Backtester {
    db: Arc<Db>,
    bars_reader: Arc<dyn BarsReader>,
    registry: Arc<DetectorRegistry>,
    detectors_cfg: Arc<DetectorsConfig>,
    /// Optional: when wired, the replay loop honors P5 event blackouts
    /// per the spec's `event_blackouts_enabled` flag. `None` skips
    /// blackouts entirely (test seam).
    event_calendar: Option<Arc<EventCalendarService>>,
    /// Optional: when wired, calibrated fill models source slippage
    /// distribution from P2 attribution. `None` falls back to the
    /// naive model with the spec's `fallback_bps`.
    tca_attribution: Option<Arc<AttributionService>>,
}

impl Backtester {
    pub fn new(
        db: Arc<Db>,
        bars_reader: Arc<dyn BarsReader>,
        registry: Arc<DetectorRegistry>,
        detectors_cfg: Arc<DetectorsConfig>,
    ) -> Self {
        Self {
            db,
            bars_reader,
            registry,
            detectors_cfg,
            event_calendar: None,
            tca_attribution: None,
        }
    }

    pub fn with_event_calendar(mut self, cal: Arc<EventCalendarService>) -> Self {
        self.event_calendar = Some(cal);
        self
    }

    pub fn with_tca_attribution(mut self, attr: Arc<AttributionService>) -> Self {
        self.tca_attribution = Some(attr);
        self
    }

    /// Validate, replay, aggregate, persist. Returns the full
    /// `BacktestResult` (including per-trade rows) on success.
    pub async fn run(&self, spec: BacktestSpec) -> Result<BacktestResult> {
        spec.validate()?;
        let spec_hash = spec.spec_hash();
        let run_id = format!("bt_{}_{}", Utc::now().timestamp_millis(), &spec_hash[..8]);
        let started_at = Utc::now();

        // Open the run row up front so a crash mid-run leaves an
        // explicit `running` / `errored` trace, not a silent gap.
        self.insert_run_row(&run_id, &spec, &spec_hash, started_at)
            .await?;

        let res = self.run_inner(&run_id, &spec_hash, &spec).await;
        match res {
            Ok(result) => {
                self.persist_result(&run_id, &result).await?;
                self.mark_run_finished(&run_id, "completed", None, result.trades.len() as i64)
                    .await?;
                Ok(result)
            }
            Err(e) => {
                let msg = format!("{e}");
                let _ = self
                    .mark_run_finished(&run_id, "errored", Some(&msg), 0)
                    .await;
                Err(e)
            }
        }
    }

    async fn run_inner(
        &self,
        run_id: &str,
        spec_hash: &str,
        spec: &BacktestSpec,
    ) -> Result<BacktestResult> {
        let mut fill_model = self.build_fill_model_for(spec).await?;
        let mut all_trades: Vec<BacktestTrade> = Vec::new();
        let mut diag = AggregateDiagnostics::default();
        let mut seq: u32 = 0;
        for symbol in &spec.symbols {
            let bars = replay::read_daily_bars(
                self.bars_reader.as_ref(),
                symbol,
                spec.date_from,
                spec.date_to_inclusive,
            )
            .await;
            if bars.is_empty() {
                warn!(
                    "backtester: no cached bars for {symbol} in {}..={} — symbol will contribute 0 trades",
                    spec.date_from, spec.date_to_inclusive
                );
                continue;
            }
            let symbol_diag = replay::replay_symbol(
                symbol,
                &bars,
                spec,
                self.detectors_cfg.as_ref(),
                self.registry.as_ref(),
                fill_model.as_mut(),
                self.event_calendar.as_ref(),
                seq,
                &mut all_trades,
            )
            .await;
            diag.n_setups_fired += symbol_diag.n_setups_fired;
            diag.n_setups_blackout_skipped += symbol_diag.n_setups_blackout_skipped;
            diag.n_setups_unsizable += symbol_diag.n_setups_unsizable;
            seq = all_trades.last().map(|t| t.seq + 1).unwrap_or(seq);
        }
        info!(
            "backtester run {run_id}: {} symbols → {} trades (fired={}, gated={}, unsizable={})",
            spec.symbols.len(),
            all_trades.len(),
            diag.n_setups_fired,
            diag.n_setups_blackout_skipped,
            diag.n_setups_unsizable,
        );
        Ok(aggregate(
            run_id,
            spec_hash,
            all_trades,
            spec.starting_equity_usd,
            diag,
        ))
    }

    async fn build_fill_model_for(&self, spec: &BacktestSpec) -> Result<Box<dyn FillModel>> {
        let rows = match (&spec.fill_model, &self.tca_attribution) {
            (
                FillModelKind::Calibrated {
                    date_from,
                    date_to_inclusive,
                    account,
                    ..
                },
                Some(attr),
            ) => {
                let acct = account.as_deref().unwrap_or("");
                let rows = attr
                    .slippage_distribution(*date_from, *date_to_inclusive, acct, None)
                    .await
                    .map_err(BacktesterError::Storage)?;
                Some(rows)
            }
            _ => None,
        };
        Ok(build_fill_model(
            &spec.fill_model,
            rows.as_deref(),
            spec.rng_seed,
        ))
    }

    async fn insert_run_row(
        &self,
        run_id: &str,
        spec: &BacktestSpec,
        spec_hash: &str,
        started_at: chrono::DateTime<Utc>,
    ) -> Result<()> {
        let run_id = run_id.to_string();
        let spec_hash = spec_hash.to_string();
        let label = spec.label.clone();
        let spec_json = serde_json::to_string(spec).map_err(|e| {
            BacktesterError::Storage(StorageError::Migration(format!("spec serialize: {e}")))
        })?;
        let started = started_at.to_rfc3339();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO backtest_runs \
                     (run_id, label, spec_json, result_json, spec_hash, n_trades, started_at, finished_at, status, error) \
                     VALUES (?1, ?2, ?3, NULL, ?4, 0, ?5, NULL, 'running', NULL)",
                    params![run_id, label, spec_json, spec_hash, started],
                )?;
                Ok(())
            })
            .await
            .map_err(BacktesterError::Storage)
    }

    async fn persist_result(&self, run_id: &str, result: &BacktestResult) -> Result<()> {
        let run_id_owned = run_id.to_string();
        // Persist the result without the inner trade list — the rows
        // table carries those, so result_json stays compact.
        let mut compact = result.clone();
        compact.trades = Vec::new();
        let result_json = serde_json::to_string(&compact).map_err(|e| {
            BacktesterError::Storage(StorageError::Migration(format!("result serialize: {e}")))
        })?;
        let trades = result.trades.clone();
        self.db
            .with_conn(move |conn| {
                let tx = conn.transaction()?;
                tx.execute(
                    "UPDATE backtest_runs SET result_json = ?1 WHERE run_id = ?2",
                    params![result_json, run_id_owned],
                )?;
                {
                    let mut stmt = tx.prepare(
                        "INSERT INTO backtest_trades \
                         (run_id, trade_seq, symbol, strategy, direction, entry_time, entry_price, \
                          exit_time, exit_price, qty, realized_r, realized_pnl, exit_reason, conviction) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    )?;
                    for t in &trades {
                        let direction = match t.direction {
                            crate::strategies::Direction::Long => "long",
                            crate::strategies::Direction::Short => "short",
                        };
                        stmt.execute(params![
                            run_id_owned,
                            t.seq as i64,
                            t.symbol,
                            t.strategy,
                            direction,
                            t.entry_time.timestamp(),
                            t.entry_price,
                            t.exit_time.timestamp(),
                            t.exit_price,
                            t.qty as i64,
                            t.realized_r,
                            t.realized_pnl,
                            t.exit_reason.as_str(),
                            t.conviction,
                        ])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(BacktesterError::Storage)
    }

    async fn mark_run_finished(
        &self,
        run_id: &str,
        status: &str,
        error: Option<&str>,
        n_trades: i64,
    ) -> Result<()> {
        let run_id = run_id.to_string();
        let status = status.to_string();
        let error = error.map(|s| s.to_string());
        let finished_at = Utc::now().to_rfc3339();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "UPDATE backtest_runs SET status = ?1, error = ?2, n_trades = ?3, finished_at = ?4 WHERE run_id = ?5",
                    params![status, error, n_trades, finished_at, run_id],
                )?;
                Ok(())
            })
            .await
            .map_err(BacktesterError::Storage)
    }

    /// Read a stored run by id. Returns `None` if missing.
    pub async fn get_run(&self, run_id: &str) -> Result<Option<BacktestResult>> {
        let run_id = run_id.to_string();
        let json: Option<String> = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT result_json FROM backtest_runs WHERE run_id = ?1 LIMIT 1")?;
                let row: Option<Option<String>> = stmt
                    .query_row(params![run_id], |r| r.get::<_, Option<String>>(0))
                    .ok();
                Ok(row.flatten())
            })
            .await
            .map_err(BacktesterError::Storage)?;
        match json {
            Some(j) => {
                let mut result: BacktestResult = serde_json::from_str(&j).map_err(|e| {
                    BacktesterError::Storage(StorageError::Migration(format!(
                        "result_json deserialize: {e}"
                    )))
                })?;
                // Hydrate trades from the rows table.
                result.trades = self.read_trades(&result.run_id).await?;
                Ok(Some(result))
            }
            None => Ok(None),
        }
    }

    async fn read_trades(&self, run_id: &str) -> Result<Vec<BacktestTrade>> {
        let run_id = run_id.to_string();
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT trade_seq, symbol, strategy, direction, entry_time, entry_price, \
                            exit_time, exit_price, qty, realized_r, realized_pnl, exit_reason, conviction \
                     FROM backtest_trades \
                     WHERE run_id = ?1 \
                     ORDER BY trade_seq ASC",
                )?;
                let rows = stmt
                    .query_map(params![run_id], |row| {
                        let seq: i64 = row.get(0)?;
                        let symbol: String = row.get(1)?;
                        let strategy: String = row.get(2)?;
                        let direction: String = row.get(3)?;
                        let entry_ts: i64 = row.get(4)?;
                        let entry_price: f64 = row.get(5)?;
                        let exit_ts: i64 = row.get(6)?;
                        let exit_price: f64 = row.get(7)?;
                        let qty: i64 = row.get(8)?;
                        let realized_r: f64 = row.get(9)?;
                        let realized_pnl: f64 = row.get(10)?;
                        let exit_reason: String = row.get(11)?;
                        let conviction: Option<String> = row.get(12)?;
                        Ok((
                            seq,
                            symbol,
                            strategy,
                            direction,
                            entry_ts,
                            entry_price,
                            exit_ts,
                            exit_price,
                            qty,
                            realized_r,
                            realized_pnl,
                            exit_reason,
                            conviction,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut out = Vec::with_capacity(rows.len());
                for (
                    seq,
                    symbol,
                    strategy,
                    direction,
                    entry_ts,
                    entry_price,
                    exit_ts,
                    exit_price,
                    qty,
                    realized_r,
                    realized_pnl,
                    exit_reason,
                    conviction,
                ) in rows
                {
                    let direction = match direction.as_str() {
                        "long" => crate::strategies::Direction::Long,
                        _ => crate::strategies::Direction::Short,
                    };
                    let entry_time = chrono::TimeZone::timestamp_opt(&Utc, entry_ts, 0)
                        .single()
                        .unwrap_or_else(Utc::now);
                    let exit_time = chrono::TimeZone::timestamp_opt(&Utc, exit_ts, 0)
                        .single()
                        .unwrap_or_else(Utc::now);
                    let exit_reason = ExitReason::parse(&exit_reason).unwrap_or(ExitReason::TimeStop);
                    out.push(BacktestTrade {
                        seq: seq as u32,
                        symbol,
                        strategy,
                        direction,
                        entry_time,
                        entry_price,
                        exit_time,
                        exit_price,
                        qty: qty as u32,
                        realized_r,
                        realized_pnl,
                        exit_reason,
                        conviction,
                    });
                }
                Ok(out)
            })
            .await
            .map_err(BacktesterError::Storage)
    }

    /// List most recent N runs. `result_json` is parsed on demand;
    /// list rows carry only the run-row fields.
    pub async fn list_runs(&self, limit: u32) -> Result<Vec<BacktestRunSummary>> {
        let limit = limit.max(1) as i64;
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT run_id, label, spec_hash, n_trades, started_at, finished_at, status, error \
                     FROM backtest_runs ORDER BY started_at DESC LIMIT ?1",
                )?;
                let rows = stmt
                    .query_map(params![limit], |r| {
                        Ok(BacktestRunSummary {
                            run_id: r.get(0)?,
                            label: r.get(1)?,
                            spec_hash: r.get(2)?,
                            n_trades: r.get(3)?,
                            started_at: r.get(4)?,
                            finished_at: r.get(5)?,
                            status: r.get(6)?,
                            error: r.get(7)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .map_err(BacktesterError::Storage)
    }

    /// Diff two runs by run_id. Reports headline metric deltas.
    /// Trades are not enumerated; the UI is expected to fetch each
    /// run separately for a deeper drill-down.
    pub async fn compare(
        &self,
        run_id_a: &str,
        run_id_b: &str,
    ) -> Result<Option<BacktestComparison>> {
        let a = self.get_run(run_id_a).await?;
        let b = self.get_run(run_id_b).await?;
        match (a, b) {
            (Some(a), Some(b)) => Ok(Some(BacktestComparison {
                run_id_a: a.run_id.clone(),
                run_id_b: b.run_id.clone(),
                a_n_trades: a.headline.n_trades,
                b_n_trades: b.headline.n_trades,
                a_pf: a.headline.profit_factor,
                b_pf: b.headline.profit_factor,
                a_expectancy_r: a.headline.expectancy_r,
                b_expectancy_r: b.headline.expectancy_r,
                a_sharpe: a.headline.sharpe,
                b_sharpe: b.headline.sharpe,
                a_max_dd: a.headline.max_dd,
                b_max_dd: b.headline.max_dd,
            })),
            _ => Ok(None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BacktestRunSummary {
    pub run_id: String,
    pub label: Option<String>,
    pub spec_hash: String,
    pub n_trades: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BacktestComparison {
    pub run_id_a: String,
    pub run_id_b: String,
    pub a_n_trades: usize,
    pub b_n_trades: usize,
    pub a_pf: f64,
    pub b_pf: f64,
    pub a_expectancy_r: f64,
    pub b_expectancy_r: f64,
    pub a_sharpe: Option<f64>,
    pub b_sharpe: Option<f64>,
    pub a_max_dd: f64,
    pub b_max_dd: f64,
}
