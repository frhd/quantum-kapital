//! Phase 8 — predictions ledger.
//!
//! Snapshots every ranked-idea (or other agent prediction) at the moment
//! it is written so the eval harness can correlate calls with eventual
//! outcomes even when the source pack is later overwritten.
//!
//! Idempotency: [`record_predictions_from_pack`] DELETEs any rows
//! already keyed to the same `(source, morning_pack_id)` before
//! re-inserting. This mirrors the upsert-on-`date` contract of
//! `agent_morning_packs` so a re-run of the morning sweep doesn't leak
//! stale predictions.

#![allow(dead_code)] // Phase 8: surface consumed by MCP tools + Tauri commands later in this phase.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::services::agent_morning_packs::AgentMorningPack;
use crate::services::research_notes::Conviction;
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

#[cfg(test)]
mod tests;

pub const SOURCE_AGENT_MORNING_SWEEP: &str = "agent_morning_sweep";

/// Internal helper alias — one tuple per ranked idea moved into the
/// `with_conn` closure: `(symbol, conviction, entry_zone, invalidation, thesis_md)`.
type IdeaTuple = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

/// Persisted form of a single prediction row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prediction {
    pub id: i64,
    pub source: String,
    pub symbol: String,
    pub conviction: Option<Conviction>,
    pub entry_zone: Option<String>,
    pub invalidation: Option<String>,
    pub target: Option<String>,
    pub thesis_md: Option<String>,
    pub morning_pack_id: Option<String>,
    pub predicted_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum PredictionError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
}

/// Replace every prediction row keyed to
/// `(source = SOURCE_AGENT_MORNING_SWEEP, morning_pack_id = pack.date)`
/// with a fresh row per ranked idea. Returns the inserted rows.
pub async fn record_predictions_from_pack(
    db: &Arc<Db>,
    pack: &AgentMorningPack,
) -> Result<Vec<Prediction>, PredictionError> {
    let pack_id = pack.date.to_string();
    let predicted_at = pack.written_at.timestamp();

    let ideas: Vec<IdeaTuple> = pack
        .ranked_ideas
        .iter()
        .map(|i| {
            (
                i.symbol.to_uppercase(),
                i.conviction.map(|c| c.as_str().to_string()),
                i.entry_zone.clone(),
                i.invalidation.clone(),
                i.thesis_md.clone(),
            )
        })
        .collect();

    let pack_id_for_db = pack_id.clone();
    let inserted_ids: Vec<i64> = db
        .with_conn(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM predictions WHERE source = ?1 AND morning_pack_id = ?2",
                rusqlite::params![SOURCE_AGENT_MORNING_SWEEP, pack_id_for_db],
            )?;
            let mut ids = Vec::with_capacity(ideas.len());
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO predictions \
                     (source, symbol, conviction, entry_zone, invalidation, target, thesis_md, \
                      morning_pack_id, predicted_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )?;
                for (symbol, conviction, entry_zone, invalidation, thesis_md) in &ideas {
                    stmt.execute(rusqlite::params![
                        SOURCE_AGENT_MORNING_SWEEP,
                        symbol,
                        conviction,
                        entry_zone,
                        invalidation,
                        Option::<String>::None, // target — not yet captured
                        thesis_md,
                        pack_id_for_db,
                        predicted_at,
                    ])?;
                    ids.push(tx.last_insert_rowid());
                }
            }
            tx.commit()?;
            Ok(ids)
        })
        .await?;

    let written_at = unix_to_utc(predicted_at);
    let rows = inserted_ids
        .into_iter()
        .zip(pack.ranked_ideas.iter())
        .map(|(id, idea)| Prediction {
            id,
            source: SOURCE_AGENT_MORNING_SWEEP.to_string(),
            symbol: idea.symbol.to_uppercase(),
            conviction: idea.conviction,
            entry_zone: idea.entry_zone.clone(),
            invalidation: idea.invalidation.clone(),
            target: None,
            thesis_md: Some(idea.thesis_md.clone()),
            morning_pack_id: Some(pack_id.clone()),
            predicted_at: written_at,
        })
        .collect();
    Ok(rows)
}

/// Lookup a single prediction by `(source, morning_pack_id, symbol)`.
/// Used by the outcome extractor to backlink an outcome to its source.
pub async fn find_for_pack(
    db: &Arc<Db>,
    morning_pack_id: &str,
    symbol: &str,
) -> Result<Option<Prediction>, PredictionError> {
    let pack_id = morning_pack_id.to_string();
    let symbol_norm = symbol.to_uppercase();
    let raw = db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT id, source, symbol, conviction, entry_zone, invalidation, target, \
                        thesis_md, morning_pack_id, predicted_at \
                 FROM predictions \
                 WHERE source = ?1 AND morning_pack_id = ?2 AND symbol = ?3",
                rusqlite::params![SOURCE_AGENT_MORNING_SWEEP, pack_id, symbol_norm],
                row_to_raw,
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;
    Ok(raw.map(decode_raw))
}

/// List predictions whose `predicted_at >= since_unix` (newest first),
/// optionally filtered by `symbol`. Used by `get_prediction_history`.
pub async fn list_predictions(
    db: &Arc<Db>,
    since_unix: i64,
    symbol: Option<&str>,
) -> Result<Vec<Prediction>, PredictionError> {
    let symbol_norm = symbol.map(|s| s.to_uppercase());
    let rows = db
        .with_conn(move |conn| match &symbol_norm {
            Some(sym) => {
                let mut stmt = conn.prepare(
                    "SELECT id, source, symbol, conviction, entry_zone, invalidation, target, \
                            thesis_md, morning_pack_id, predicted_at \
                     FROM predictions \
                     WHERE predicted_at >= ?1 AND symbol = ?2 \
                     ORDER BY predicted_at DESC, id DESC",
                )?;
                let raws = stmt
                    .query_map(rusqlite::params![since_unix, sym], row_to_raw)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(raws)
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, source, symbol, conviction, entry_zone, invalidation, target, \
                            thesis_md, morning_pack_id, predicted_at \
                     FROM predictions \
                     WHERE predicted_at >= ?1 \
                     ORDER BY predicted_at DESC, id DESC",
                )?;
                let raws = stmt
                    .query_map(rusqlite::params![since_unix], row_to_raw)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(raws)
            }
        })
        .await?;
    Ok(rows.into_iter().map(decode_raw).collect())
}

type RawRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);

fn row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
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
    ))
}

fn decode_raw(r: RawRow) -> Prediction {
    let (
        id,
        source,
        symbol,
        conviction_s,
        entry_zone,
        invalidation,
        target,
        thesis_md,
        morning_pack_id,
        predicted_at,
    ) = r;
    Prediction {
        id,
        source,
        symbol,
        conviction: conviction_s.as_deref().and_then(Conviction::parse),
        entry_zone,
        invalidation,
        target,
        thesis_md,
        morning_pack_id,
        predicted_at: unix_to_utc(predicted_at),
    }
}
