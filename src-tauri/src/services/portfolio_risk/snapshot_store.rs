//! Phase 8 — `portfolio_snapshots` + `bracket_groups` reads.
//! Wraps the rusqlite plumbing so the rest of the module is DB-free.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::error::StorageError;
use crate::storage::Db;

use super::exposure::PortfolioRisk;
use super::Result;

/// One persisted row from `portfolio_snapshots`. Carries the
/// snapshot summary (NLV, total dollar-risk, count) plus the full
/// `exposures_json` payload so the UI can replay the historical
/// view without recomputing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSnapshotRow {
    pub id: i64,
    pub account: String,
    pub at: DateTime<Utc>,
    pub nlv_cents: i64,
    pub total_dollar_risk_cents: i64,
    pub open_position_count: usize,
    pub exposures_json: serde_json::Value,
}

#[derive(Clone)]
pub struct SnapshotStore {
    db: Arc<Db>,
}

impl SnapshotStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Read open-bracket stop prices keyed by symbol for `account`.
    /// Used by the snapshot path to join positions with their stop
    /// distance. Returns `(symbol → stop_cents)`. Symbols without a
    /// recorded stop are absent from the map (the snapshot path
    /// falls back to a 5% estimate).
    ///
    /// Filters to `last_status IN ('open', 'partial')` so closed
    /// brackets don't double-count. When multiple brackets exist
    /// for the same symbol (split entries), the most-recent stop
    /// wins.
    pub async fn open_bracket_stops(&self, account: &str) -> Result<HashMap<String, i64>> {
        let account = account.to_string();
        let rows = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT symbol, stop_price_cents \
                     FROM bracket_groups \
                     WHERE account = ?1 AND last_status IN ('open', 'partial') \
                     ORDER BY last_status_at DESC",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![account], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(StorageError::from)?;
                Ok(rows)
            })
            .await?;
        let mut map = HashMap::new();
        for (symbol, stop) in rows {
            map.entry(symbol.to_uppercase()).or_insert(stop);
        }
        Ok(map)
    }

    /// Persist a snapshot row. Returns the new row id so the caller
    /// can echo it back to listeners via `PortfolioRiskChanged`.
    pub async fn insert(
        &self,
        account: &str,
        at: DateTime<Utc>,
        snapshot: &PortfolioRisk,
    ) -> Result<i64> {
        let payload = serde_json::json!({
            "by_sector": snapshot.by_sector,
            "by_factor": snapshot.by_factor,
            "by_name": snapshot.open_positions.iter().map(|p| {
                serde_json::json!({
                    "symbol": p.symbol,
                    "dollar_risk_cents": p.dollar_risk_cents,
                    "stop_estimated": p.stop_estimated,
                    "sector": p.sector,
                })
            }).collect::<Vec<_>>(),
        });
        let payload_str = serde_json::to_string(&payload)?;
        let account = account.to_string();
        let at_unix = at.timestamp();
        let nlv = snapshot.nlv_cents;
        let total = snapshot.total_dollar_risk_cents;
        let count = snapshot.open_positions.len() as i64;

        let id = self
            .db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO portfolio_snapshots \
                       (account, at_unix, nlv_cents, total_dollar_risk_cents, \
                        open_position_count, exposures_json) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![account, at_unix, nlv, total, count, payload_str],
                )
                .map_err(StorageError::from)?;
                Ok(conn.last_insert_rowid())
            })
            .await?;
        Ok(id)
    }

    /// Read the most-recent `limit` snapshots for `account`,
    /// newest-first. The UI's history chart eats this directly.
    pub async fn list(&self, account: &str, limit: u32) -> Result<Vec<PortfolioSnapshotRow>> {
        let account = account.to_string();
        let limit = limit as i64;
        let rows = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, account, at_unix, nlv_cents, total_dollar_risk_cents, \
                            open_position_count, exposures_json \
                     FROM portfolio_snapshots \
                     WHERE account = ?1 \
                     ORDER BY at_unix DESC LIMIT ?2",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![account, limit], |row| {
                        Ok(SnapshotRowRaw {
                            id: row.get(0)?,
                            account: row.get(1)?,
                            at_unix: row.get(2)?,
                            nlv_cents: row.get(3)?,
                            total_dollar_risk_cents: row.get(4)?,
                            open_position_count: row.get::<_, i64>(5)?.max(0) as usize,
                            exposures_json: row.get(6)?,
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

struct SnapshotRowRaw {
    id: i64,
    account: String,
    at_unix: i64,
    nlv_cents: i64,
    total_dollar_risk_cents: i64,
    open_position_count: usize,
    exposures_json: String,
}

fn decode_row(r: SnapshotRowRaw) -> Result<PortfolioSnapshotRow> {
    let exposures_json: serde_json::Value = serde_json::from_str(&r.exposures_json)?;
    Ok(PortfolioSnapshotRow {
        id: r.id,
        account: r.account,
        at: DateTime::<Utc>::from_timestamp(r.at_unix, 0).unwrap_or_else(Utc::now),
        nlv_cents: r.nlv_cents,
        total_dollar_risk_cents: r.total_dollar_risk_cents,
        open_position_count: r.open_position_count,
        exposures_json,
    })
}
