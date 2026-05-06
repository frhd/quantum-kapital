//! Phase 9 — `regime_snapshots` reads + writes. Wraps the rusqlite
//! plumbing so the rest of the module is DB-free.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::error::StorageError;
use crate::storage::Db;

use super::inputs::RegimeInputs;
use super::types::{Regime, SnapshotSource};
use super::Result;

/// One persisted row from `regime_snapshots`. Both raw and stable
/// are persisted so a gate decision is replayable without re-running
/// the persistence rule against the prior history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeSnapshotRow {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub raw: Regime,
    pub stable: Regime,
    pub inputs_json: serde_json::Value,
    pub source: String,
}

/// Internal payload for `regime_json`. Public so consumers that
/// receive a serde_json::Value can decode without depending on the
/// rusqlite layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeRowPayload {
    pub raw: Regime,
    pub stable: Regime,
}

#[derive(Clone)]
pub struct SnapshotStore {
    db: Arc<Db>,
}

impl SnapshotStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Insert one snapshot row. Returns the new id.
    pub async fn insert(
        &self,
        at: DateTime<Utc>,
        raw: &Regime,
        stable: &Regime,
        inputs: &RegimeInputs,
        source: SnapshotSource,
    ) -> Result<i64> {
        let payload = RegimeRowPayload {
            raw: *raw,
            stable: *stable,
        };
        let regime_str = serde_json::to_string(&payload)?;
        let inputs_str = serde_json::to_string(inputs)?;
        let source_str = source.as_str().to_string();
        let at_unix = at.timestamp();
        let id = self
            .db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO regime_snapshots (at_unix, regime_json, inputs_json, source) \
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![at_unix, regime_str, inputs_str, source_str],
                )
                .map_err(StorageError::from)?;
                Ok(conn.last_insert_rowid())
            })
            .await?;
        Ok(id)
    }

    /// Read the most-recent `limit` snapshots, newest-first. The UI's
    /// timeline + the persistence rule both eat this directly.
    pub async fn list(&self, limit: u32) -> Result<Vec<RegimeSnapshotRow>> {
        let limit = limit.clamp(1, 1000) as i64;
        let rows = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, at_unix, regime_json, inputs_json, source \
                     FROM regime_snapshots \
                     ORDER BY at_unix DESC, id DESC LIMIT ?1",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![limit], |row| {
                        let id: i64 = row.get(0)?;
                        let at_unix: i64 = row.get(1)?;
                        let regime_json: String = row.get(2)?;
                        let inputs_json: String = row.get(3)?;
                        let source: String = row.get(4)?;
                        Ok(RowRaw {
                            id,
                            at_unix,
                            regime_json,
                            inputs_json,
                            source,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(StorageError::from)?;
                Ok(rows)
            })
            .await?;
        rows.into_iter().map(decode_row).collect()
    }

    /// Same as `list` but filtered to a specific source. Used to read
    /// only the canonical daily-close history when applying the
    /// 3-day persistence rule (intraday ticks shouldn't trip the
    /// rule on their own).
    pub async fn list_by_source(
        &self,
        source: SnapshotSource,
        limit: u32,
    ) -> Result<Vec<RegimeSnapshotRow>> {
        let limit = limit.clamp(1, 1000) as i64;
        let source_str = source.as_str().to_string();
        let rows = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, at_unix, regime_json, inputs_json, source \
                     FROM regime_snapshots \
                     WHERE source = ?1 \
                     ORDER BY at_unix DESC, id DESC LIMIT ?2",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![source_str, limit], |row| {
                        let id: i64 = row.get(0)?;
                        let at_unix: i64 = row.get(1)?;
                        let regime_json: String = row.get(2)?;
                        let inputs_json: String = row.get(3)?;
                        let source: String = row.get(4)?;
                        Ok(RowRaw {
                            id,
                            at_unix,
                            regime_json,
                            inputs_json,
                            source,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(StorageError::from)?;
                Ok(rows)
            })
            .await?;
        rows.into_iter().map(decode_row).collect()
    }
}

struct RowRaw {
    id: i64,
    at_unix: i64,
    regime_json: String,
    inputs_json: String,
    source: String,
}

fn decode_row(r: RowRaw) -> Result<RegimeSnapshotRow> {
    let payload: RegimeRowPayload = serde_json::from_str(&r.regime_json)?;
    let inputs_json: serde_json::Value = serde_json::from_str(&r.inputs_json)?;
    Ok(RegimeSnapshotRow {
        id: r.id,
        at: DateTime::<Utc>::from_timestamp(r.at_unix, 0).unwrap_or_else(Utc::now),
        raw: payload.raw,
        stable: payload.stable,
        inputs_json,
        source: r.source,
    })
}
