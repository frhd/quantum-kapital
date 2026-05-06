//! Phase 10 — `param_vintages` reads + writes. Wraps the rusqlite
//! plumbing so the rest of the module stays DB-free.
//!
//! Schema lives in `V26__param_refit.sql`. The store enforces the
//! "active = most recent non-superseded row per detector" invariant
//! at write time: every successful `lock_new` stamps the prior
//! active row's `superseded_at` and inserts the new winner with
//! `superseded_at = NULL`, inside a single transaction.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::strategies::DetectorsConfig;

use super::sweep::SweepCandidate;
use super::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockSource {
    /// Monthly cron — the routine cadence.
    Cron,
    /// Operator-initiated `lock_manual` override.
    Manual,
    /// Startup backfill: detector had no active vintage so the
    /// service ran a one-shot refit.
    Backfill,
}

impl LockSource {
    pub fn as_str(self) -> &'static str {
        match self {
            LockSource::Cron => "cron",
            LockSource::Manual => "manual",
            LockSource::Backfill => "backfill",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cron" => Some(LockSource::Cron),
            "manual" => Some(LockSource::Manual),
            "backfill" => Some(LockSource::Backfill),
            _ => None,
        }
    }
}

/// One persisted row from `param_vintages`. Active vintages have
/// `superseded_at = None`; historical (replaced) vintages carry the
/// supersede timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamVintage {
    pub vintage_id: String,
    pub detector: String,
    pub params_json: serde_json::Value,
    pub objective_value: f64,
    pub oos_n_trades: i64,
    pub train_window_from: NaiveDate,
    pub train_window_to: NaiveDate,
    pub oos_window_from: NaiveDate,
    pub oos_window_to: NaiveDate,
    pub locked_at: DateTime<Utc>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub source: String,
    /// Audit trail: every config the sweep tried with its score
    /// shape. Empty for `manual` / `backfill`-without-sweep rows.
    pub attempted_configs_json: serde_json::Value,
    pub notes: Option<String>,
}

#[derive(Clone)]
pub struct VintageStore {
    db: Arc<Db>,
}

impl VintageStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Most recent non-superseded vintage for `detector`, or `None`.
    pub async fn active_for(&self, detector: &str) -> Result<Option<ParamVintage>> {
        let detector = detector.to_string();
        let row: Option<RowRaw> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT vintage_id, detector, params_json, objective_value, oos_n_trades, \
                            train_window_from, train_window_to, oos_window_from, oos_window_to, \
                            locked_at, superseded_at, source, attempted_configs_json, notes \
                     FROM param_vintages \
                     WHERE detector = ?1 AND superseded_at IS NULL \
                     ORDER BY locked_at DESC LIMIT 1",
                    rusqlite::params![detector],
                    raw_from_row,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        match row {
            Some(r) => Ok(Some(decode_row(r)?)),
            None => Ok(None),
        }
    }

    /// One row per detector that has an active vintage. Order is
    /// detector-ASC for stable callsite consumption.
    pub async fn active_all(&self) -> Result<Vec<ParamVintage>> {
        let raws = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT vintage_id, detector, params_json, objective_value, oos_n_trades, \
                            train_window_from, train_window_to, oos_window_from, oos_window_to, \
                            locked_at, superseded_at, source, attempted_configs_json, notes \
                     FROM param_vintages \
                     WHERE superseded_at IS NULL \
                     ORDER BY detector ASC",
                )?;
                let rows = stmt
                    .query_map([], raw_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(StorageError::from)?;
                Ok(rows)
            })
            .await?;
        raws.into_iter().map(decode_row).collect()
    }

    /// Vintage history for `detector`, newest-first. Includes both
    /// active and superseded rows.
    pub async fn history_for(&self, detector: &str, limit: u32) -> Result<Vec<ParamVintage>> {
        let detector = detector.to_string();
        let limit = limit.clamp(1, 1000) as i64;
        let raws = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT vintage_id, detector, params_json, objective_value, oos_n_trades, \
                            train_window_from, train_window_to, oos_window_from, oos_window_to, \
                            locked_at, superseded_at, source, attempted_configs_json, notes \
                     FROM param_vintages \
                     WHERE detector = ?1 \
                     ORDER BY locked_at DESC LIMIT ?2",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![detector, limit], raw_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(StorageError::from)?;
                Ok(rows)
            })
            .await?;
        raws.into_iter().map(decode_row).collect()
    }

    /// Look up one vintage by id. Returns `None` if missing.
    pub async fn get(&self, vintage_id: &str) -> Result<Option<ParamVintage>> {
        let vintage_id = vintage_id.to_string();
        let row: Option<RowRaw> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT vintage_id, detector, params_json, objective_value, oos_n_trades, \
                            train_window_from, train_window_to, oos_window_from, oos_window_to, \
                            locked_at, superseded_at, source, attempted_configs_json, notes \
                     FROM param_vintages WHERE vintage_id = ?1 LIMIT 1",
                    rusqlite::params![vintage_id],
                    raw_from_row,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        match row {
            Some(r) => Ok(Some(decode_row(r)?)),
            None => Ok(None),
        }
    }

    /// Atomic supersede + insert. The prior active vintage (if any)
    /// is stamped `superseded_at = locked_at_unix`; the new winner
    /// is inserted with `superseded_at = NULL`.
    #[allow(clippy::too_many_arguments)]
    pub async fn lock_new(
        &self,
        detector: &str,
        params_json: &serde_json::Value,
        objective_value: f64,
        oos_n_trades: i64,
        inputs: &super::SweepInputs,
        locked_at: DateTime<Utc>,
        source: LockSource,
        attempted_configs: &[SweepCandidate],
        notes: Option<String>,
    ) -> Result<ParamVintage> {
        let detector_owned = detector.to_string();
        let vintage_id = generate_vintage_id(detector, params_json, locked_at);
        let params_str = serde_json::to_string(params_json)?;
        let attempted: Vec<AttemptedConfigRecord> = attempted_configs
            .iter()
            .map(AttemptedConfigRecord::from_sweep_candidate)
            .collect();
        let attempted_str = serde_json::to_string(&attempted)?;
        let train_from = inputs.train_from.format("%Y-%m-%d").to_string();
        let train_to = inputs.train_to.format("%Y-%m-%d").to_string();
        let oos_from = inputs.oos_from.format("%Y-%m-%d").to_string();
        let oos_to = inputs.oos_to.format("%Y-%m-%d").to_string();
        let locked_unix = locked_at.timestamp();
        let source_str = source.as_str().to_string();
        let vintage_id_for_db = vintage_id.clone();
        let notes_for_db = notes.clone();
        let detector_for_db = detector_owned.clone();
        self.db
            .with_conn(move |conn| {
                let tx = conn.transaction().map_err(StorageError::from)?;
                tx.execute(
                    "UPDATE param_vintages SET superseded_at = ?1 \
                     WHERE detector = ?2 AND superseded_at IS NULL",
                    rusqlite::params![locked_unix, detector_for_db],
                )
                .map_err(StorageError::from)?;
                tx.execute(
                    "INSERT INTO param_vintages \
                       (vintage_id, detector, params_json, objective_value, oos_n_trades, \
                        train_window_from, train_window_to, oos_window_from, oos_window_to, \
                        locked_at, superseded_at, source, attempted_configs_json, notes) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12, ?13)",
                    rusqlite::params![
                        vintage_id_for_db,
                        detector_for_db,
                        params_str,
                        objective_value,
                        oos_n_trades,
                        train_from,
                        train_to,
                        oos_from,
                        oos_to,
                        locked_unix,
                        source_str,
                        attempted_str,
                        notes_for_db,
                    ],
                )
                .map_err(StorageError::from)?;
                tx.commit().map_err(StorageError::from)?;
                Ok(())
            })
            .await?;
        self.get(&vintage_id).await?.ok_or_else(|| {
            StorageError::Migration(format!("vintage {vintage_id} disappeared after insert")).into()
        })
    }
}

/// Apply a vintage's params to the supplied [`DetectorsConfig`],
/// in-place. Returns an error if the params JSON doesn't deserialize
/// into the detector's expected shape — caller logs and falls back
/// to the bounds config.
pub fn apply_vintage_to_config(
    cfg: &mut DetectorsConfig,
    vintage: &ParamVintage,
) -> std::result::Result<(), serde_json::Error> {
    use crate::strategies::{BreakoutCfg, EpisodicPivotCfg, ParabolicShortCfg};
    match vintage.detector.as_str() {
        super::sweep::BREAKOUT_DETECTOR => {
            let parsed: BreakoutCfg = serde_json::from_value(vintage.params_json.clone())?;
            cfg.breakout = parsed;
        }
        super::sweep::EPISODIC_PIVOT_DETECTOR => {
            let parsed: EpisodicPivotCfg = serde_json::from_value(vintage.params_json.clone())?;
            cfg.episodic_pivot = parsed;
        }
        super::sweep::PARABOLIC_SHORT_DETECTOR => {
            let parsed: ParabolicShortCfg = serde_json::from_value(vintage.params_json.clone())?;
            cfg.parabolic_short = parsed;
        }
        _ => {
            // Unknown detector: nothing to apply. Caller treats as no-op.
        }
    }
    Ok(())
}

fn generate_vintage_id(
    detector: &str,
    params_json: &serde_json::Value,
    locked_at: DateTime<Utc>,
) -> String {
    use chrono::Datelike;
    let date_part = locked_at.format("%Y%m%d");
    // FNV-1a 64-bit over (detector + canonical params + locked_at_ms).
    // Including the timestamp makes two same-day, same-params locks
    // distinct (e.g., backfill + cron on day one).
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let canon = format!(
        "{}|{}|{}",
        detector,
        serde_json::to_string(params_json).unwrap_or_default(),
        locked_at.timestamp_millis(),
    );
    let mut h: u64 = FNV_OFFSET;
    for b in canon.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    let _ = locked_at.year(); // silence unused-import lint when chrono::Datelike is dragged in via formatter
    format!("vint_{}_{}_{:08x}", detector, date_part, (h >> 32) as u32)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttemptedConfigRecord {
    params: serde_json::Value,
    /// Objective value when constraints passed; `None` when the
    /// candidate was rejected.
    score: Option<f64>,
    n_trades: usize,
    constraint_failures: Vec<String>,
}

impl AttemptedConfigRecord {
    fn from_sweep_candidate(c: &SweepCandidate) -> Self {
        let n_trades = c.score.as_ref().map(|s| s.n_trades).unwrap_or(0);
        let value = c.score.as_ref().map(|s| s.value);
        let constraint_failures = c
            .constraint_failures
            .iter()
            .map(|f| f.as_str().to_string())
            .collect();
        Self {
            params: c.params_json.clone(),
            score: value,
            n_trades,
            constraint_failures,
        }
    }
}

struct RowRaw {
    vintage_id: String,
    detector: String,
    params_json: String,
    objective_value: f64,
    oos_n_trades: i64,
    train_window_from: String,
    train_window_to: String,
    oos_window_from: String,
    oos_window_to: String,
    locked_at: i64,
    superseded_at: Option<i64>,
    source: String,
    attempted_configs_json: String,
    notes: Option<String>,
}

fn raw_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RowRaw> {
    Ok(RowRaw {
        vintage_id: row.get(0)?,
        detector: row.get(1)?,
        params_json: row.get(2)?,
        objective_value: row.get(3)?,
        oos_n_trades: row.get(4)?,
        train_window_from: row.get(5)?,
        train_window_to: row.get(6)?,
        oos_window_from: row.get(7)?,
        oos_window_to: row.get(8)?,
        locked_at: row.get(9)?,
        superseded_at: row.get(10)?,
        source: row.get(11)?,
        attempted_configs_json: row.get(12)?,
        notes: row.get(13)?,
    })
}

fn decode_row(r: RowRaw) -> Result<ParamVintage> {
    let params_json: serde_json::Value = serde_json::from_str(&r.params_json)?;
    let attempted_configs_json: serde_json::Value =
        serde_json::from_str(&r.attempted_configs_json)?;
    Ok(ParamVintage {
        vintage_id: r.vintage_id,
        detector: r.detector,
        params_json,
        objective_value: r.objective_value,
        oos_n_trades: r.oos_n_trades,
        train_window_from: parse_iso_date(&r.train_window_from)?,
        train_window_to: parse_iso_date(&r.train_window_to)?,
        oos_window_from: parse_iso_date(&r.oos_window_from)?,
        oos_window_to: parse_iso_date(&r.oos_window_to)?,
        locked_at: DateTime::<Utc>::from_timestamp(r.locked_at, 0).unwrap_or_else(Utc::now),
        superseded_at: r
            .superseded_at
            .map(|u| DateTime::<Utc>::from_timestamp(u, 0).unwrap_or_else(Utc::now)),
        source: r.source,
        attempted_configs_json,
        notes: r.notes,
    })
}

fn parse_iso_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| StorageError::Migration(format!("invalid date '{s}': {e}")).into())
}
