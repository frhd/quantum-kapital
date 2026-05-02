//! Phase 8 — eval harness aggregation service.
//!
//! One service that composes `predictions`, `outcomes`, and `llm_calls`
//! into the three roll-ups the eval dashboard / MCP tools expose:
//!
//! 1. [`calibration_stats`] — per-conviction outcome counts + win rate
//!    over a windowed timeframe.
//! 2. [`prediction_history`] — timeline of (prediction, outcome?) pairs
//!    for a single symbol so the agent can self-introspect when picking
//!    the same ticker again.
//! 3. [`cost_attribution`] — `llm_calls` rolled up by attribution bucket
//!    (`loop_name` if set, else `kind:<llm_kind>`) plus cost-per-A-call
//!    so the dashboard can answer "is the agent expensive vs the value
//!    it delivers?".
//!
//! All functions take an explicit `since_unix` so callers (and tests)
//! can pin the window without leaking real wall-clock behaviour.

#![allow(dead_code)] // surface consumed by Phase 8 MCP tools + Tauri commands.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::services::outcome_extractor::{OutcomeClass, OutcomeRow};
use crate::services::predictions::Prediction;
use crate::services::research_notes::Conviction;
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

#[cfg(test)]
mod tests;

#[derive(Debug, Error)]
pub enum EvalError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
}

/// Per-conviction outcome rollup. `conviction = None` collects ideas the
/// agent left ungraded; the `overall` bucket aggregates everything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvictionBucket {
    pub conviction: Option<String>,
    pub total: i64,
    pub hit_target: i64,
    pub hit_entry: i64,
    pub hit_invalidation: i64,
    pub drifted: i64,
    pub no_movement: i64,
    pub skipped: i64,
    pub unparseable: i64,
    /// (`hit_target` + `hit_entry`) / scoreable_total.
    /// `scoreable_total` excludes `skipped` + `unparseable` so prompt
    /// coverage gaps don't poison the rate. `0.0` when scoreable_total
    /// is zero (caller should treat as "no data").
    pub win_rate: f64,
    /// hit_target / scoreable_total — the harder bar (full follow-through).
    pub target_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationStats {
    pub window_days: i64,
    pub since_unix: i64,
    pub buckets: Vec<ConvictionBucket>,
    pub overall: ConvictionBucket,
}

/// Per-bucket cost rollup. `bucket` is the attribution label —
/// `loop_name` when set on the row, else `"kind:<llm_kind>"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostBucket {
    pub bucket: String,
    pub call_count: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostAttribution {
    pub window_days: i64,
    pub since_unix: i64,
    pub total_cost_usd: f64,
    pub total_calls: i64,
    pub buckets: Vec<CostBucket>,
    /// Count of `predictions` rows in window with `conviction = 'A'`.
    pub a_conviction_count: i64,
    /// `total_cost_usd / a_conviction_count`. `f64::NAN` when no
    /// A-conviction calls — JSON-serialized as null by serde_json so
    /// the UI gets a clean "no data" signal.
    pub usd_per_a_conviction: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictionWithOutcome {
    pub prediction: Prediction,
    pub outcome: Option<OutcomeRow>,
}

/// Roll up `outcomes` joined to `predictions` over the time window
/// `predicted_at >= since_unix`. Outcomes attached to predictions
/// outside the window are dropped.
pub async fn calibration_stats(
    db: &Arc<Db>,
    window_days: i64,
    since_unix: i64,
) -> Result<CalibrationStats, EvalError> {
    let rows: Vec<(Option<String>, String, i64)> = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT p.conviction, o.outcome_class, COUNT(*) \
                 FROM outcomes o \
                 INNER JOIN predictions p ON p.id = o.prediction_id \
                 WHERE p.predicted_at >= ?1 \
                 GROUP BY p.conviction, o.outcome_class",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![since_unix], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;

    let mut a = empty_bucket(Some("A"));
    let mut b = empty_bucket(Some("B"));
    let mut c = empty_bucket(Some("C"));
    let mut none = empty_bucket(None);

    for (conviction_s, class_s, count) in &rows {
        let class = OutcomeClass::parse(class_s).unwrap_or(OutcomeClass::Unparseable);
        let bucket = match conviction_s.as_deref().and_then(Conviction::parse) {
            Some(Conviction::A) => &mut a,
            Some(Conviction::B) => &mut b,
            Some(Conviction::C) => &mut c,
            None => &mut none,
        };
        accumulate(bucket, class, *count);
    }

    finalize_rates(&mut a);
    finalize_rates(&mut b);
    finalize_rates(&mut c);
    finalize_rates(&mut none);

    let mut overall = empty_bucket(None);
    overall.conviction = Some("overall".to_string());
    for src in [&a, &b, &c, &none] {
        overall.total += src.total;
        overall.hit_target += src.hit_target;
        overall.hit_entry += src.hit_entry;
        overall.hit_invalidation += src.hit_invalidation;
        overall.drifted += src.drifted;
        overall.no_movement += src.no_movement;
        overall.skipped += src.skipped;
        overall.unparseable += src.unparseable;
    }
    finalize_rates(&mut overall);

    Ok(CalibrationStats {
        window_days,
        since_unix,
        buckets: vec![a, b, c, none],
        overall,
    })
}

fn empty_bucket(label: Option<&str>) -> ConvictionBucket {
    ConvictionBucket {
        conviction: label.map(str::to_string),
        total: 0,
        hit_target: 0,
        hit_entry: 0,
        hit_invalidation: 0,
        drifted: 0,
        no_movement: 0,
        skipped: 0,
        unparseable: 0,
        win_rate: 0.0,
        target_rate: 0.0,
    }
}

fn accumulate(bucket: &mut ConvictionBucket, class: OutcomeClass, count: i64) {
    bucket.total += count;
    match class {
        OutcomeClass::HitTarget => bucket.hit_target += count,
        OutcomeClass::HitEntry => bucket.hit_entry += count,
        OutcomeClass::HitInvalidation => bucket.hit_invalidation += count,
        OutcomeClass::Drifted => bucket.drifted += count,
        OutcomeClass::NoMovement => bucket.no_movement += count,
        OutcomeClass::Skipped => bucket.skipped += count,
        OutcomeClass::Unparseable => bucket.unparseable += count,
    }
}

fn finalize_rates(bucket: &mut ConvictionBucket) {
    let scoreable = bucket.total - bucket.skipped - bucket.unparseable;
    if scoreable > 0 {
        let wins = bucket.hit_target + bucket.hit_entry;
        bucket.win_rate = wins as f64 / scoreable as f64;
        bucket.target_rate = bucket.hit_target as f64 / scoreable as f64;
    }
}

/// Cost rollup over `called_at >= since_unix`. Buckets are
/// `loop_name` when present, else `"kind:<kind>"` so historical rows
/// (no `loop_name`) still show up.
pub async fn cost_attribution(
    db: &Arc<Db>,
    window_days: i64,
    since_unix: i64,
) -> Result<CostAttribution, EvalError> {
    let buckets: Vec<CostBucket> = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT COALESCE(loop_name, 'kind:' || kind) AS bucket, \
                        COUNT(*), \
                        COALESCE(SUM(cost_usd), 0.0) \
                 FROM llm_calls \
                 WHERE called_at >= ?1 \
                 GROUP BY bucket \
                 ORDER BY SUM(cost_usd) DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![since_unix], |r| {
                    Ok(CostBucket {
                        bucket: r.get::<_, String>(0)?,
                        call_count: r.get::<_, i64>(1)?,
                        cost_usd: r.get::<_, f64>(2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;

    let total_cost_usd = buckets.iter().map(|b| b.cost_usd).sum();
    let total_calls = buckets.iter().map(|b| b.call_count).sum();

    let a_conviction_count: i64 = db
        .with_conn(move |conn| {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM predictions \
                 WHERE conviction = 'A' AND predicted_at >= ?1",
                rusqlite::params![since_unix],
                |r| r.get(0),
            )?;
            Ok(n)
        })
        .await?;

    let usd_per_a_conviction = if a_conviction_count > 0 {
        total_cost_usd / a_conviction_count as f64
    } else {
        f64::NAN
    };

    Ok(CostAttribution {
        window_days,
        since_unix,
        total_cost_usd,
        total_calls,
        buckets,
        a_conviction_count,
        usd_per_a_conviction,
    })
}

/// All predictions for `symbol` with `predicted_at >= since_unix`,
/// joined with their outcome row when one exists, newest-first.
pub async fn prediction_history(
    db: &Arc<Db>,
    symbol: &str,
    since_unix: i64,
) -> Result<Vec<PredictionWithOutcome>, EvalError> {
    let symbol_norm = symbol.to_uppercase();
    let preds = crate::services::predictions::list_predictions(db, since_unix, Some(&symbol_norm))
        .await
        .map_err(|e| match e {
            crate::services::predictions::PredictionError::Storage(s) => EvalError::Storage(s),
        })?;

    let mut out = Vec::with_capacity(preds.len());
    for p in preds {
        let pid = p.id;
        let outcome = lookup_outcome_for_prediction(db, pid).await?;
        out.push(PredictionWithOutcome {
            prediction: p,
            outcome,
        });
    }
    Ok(out)
}

async fn lookup_outcome_for_prediction(
    db: &Arc<Db>,
    prediction_id: i64,
) -> Result<Option<OutcomeRow>, EvalError> {
    let raw: Option<RawOutcomeRow> = db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT id, pack_date, symbol, outcome_class, conviction, \
                        entry_zone_low, entry_zone_high, invalidation_lvl, \
                        realized_high, realized_low, realized_close, \
                        eval_window_days, evaluated_at, prediction_id \
                 FROM outcomes WHERE prediction_id = ?1",
                rusqlite::params![prediction_id],
                row_to_raw_outcome,
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;
    Ok(raw.map(decode_outcome).transpose()?)
}

type RawOutcomeRow = (
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    f64,
    f64,
    f64,
    i64,
    i64,
    Option<i64>,
);

fn row_to_raw_outcome(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawOutcomeRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
    ))
}

fn decode_outcome(r: RawOutcomeRow) -> Result<OutcomeRow, EvalError> {
    let (
        id,
        pack_date_s,
        symbol,
        outcome_s,
        conviction_s,
        entry_lo,
        entry_hi,
        inv,
        hi,
        lo,
        close,
        window,
        evaluated_at,
        prediction_id,
    ) = r;
    let pack_date: NaiveDate = NaiveDate::parse_from_str(&pack_date_s, "%Y-%m-%d")
        .map_err(|e| EvalError::Storage(StorageError::Migration(format!("invalid pack_date '{pack_date_s}': {e}"))))?;
    let outcome_class = OutcomeClass::parse(&outcome_s).ok_or_else(|| {
        EvalError::Storage(StorageError::Migration(format!(
            "invalid outcome_class '{outcome_s}'"
        )))
    })?;
    let conviction = conviction_s.as_deref().and_then(Conviction::parse);
    let evaluated_at: DateTime<Utc> = unix_to_utc(evaluated_at);
    Ok(OutcomeRow {
        id,
        pack_date,
        symbol,
        outcome_class,
        conviction,
        entry_zone_low: entry_lo,
        entry_zone_high: entry_hi,
        invalidation_lvl: inv,
        realized_high: hi,
        realized_low: lo,
        realized_close: close,
        eval_window_days: window,
        evaluated_at,
        prediction_id,
    })
}
