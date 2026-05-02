//! Phase 4 — SQLite-backed store for operator-curated fundamentals.
//!
//! The `manual_fundamentals` table is written by the MCP
//! `set_fundamentals` tool (see [`crate::mcp::tools::set_fundamentals`])
//! and read first by [`crate::services::fundamentals_provider::composite::CompositeFundamentalsProvider`].
//! A row here always wins over the AV cache + AV API for that symbol.
//!
//! The module mirrors `services::mcp_audit` / `services::research_notes`:
//! free functions over `&Arc<Db>` are wrapped in a thin struct so call
//! sites can hold an `Arc<ManualFundamentalsStore>` and unit-test through
//! the same on-disk path as production.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ibkr::types::FundamentalData;
use crate::storage::error::StorageError;
use crate::storage::Db;

#[cfg(test)]
mod tests;

/// Persisted row in `manual_fundamentals`. The serialized `FundamentalData`
/// is stored as JSON text rather than blob columns so a future migration
/// to the trait's contract is a simple JSON re-serialize, not a schema
/// change. The serde-round-trip cost (~microseconds) is irrelevant on
/// the read path because the store is consulted at most once per
/// `get_fundamentals` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualFundamentalsRow {
    pub symbol: String,
    pub as_of_date: String,
    pub source: String,
    pub data: FundamentalData,
    /// Unix seconds — wall-clock time the row landed. Mirrors
    /// `mcp_audit.called_at` so post-hoc audit joins on `(written_at,
    /// written_by, symbol)` are cheap.
    pub written_at: i64,
    pub written_by: String,
}

/// Outcome of an `upsert` call. The `prior` field exposes the previous
/// row (if any) so the MCP tool's response can render a diff for the
/// LLM and the user.
#[derive(Debug, Clone)]
pub struct UpsertOutcome {
    pub current: ManualFundamentalsRow,
    pub prior: Option<ManualFundamentalsRow>,
}

/// Thin wrapper around `Arc<Db>` exposing the manual-fundamentals API.
/// Cloneable; instances share the same SQLite pool.
#[derive(Clone)]
pub struct ManualFundamentalsStore {
    db: Arc<Db>,
}

impl ManualFundamentalsStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Read the manual row for `symbol`, if present. Symbol is matched
    /// case-insensitively (uppercased before query).
    pub async fn get(&self, symbol: &str) -> Result<Option<ManualFundamentalsRow>, StorageError> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Ok(None);
        }
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT symbol, as_of_date, source, payload_json, written_at, written_by \
                     FROM manual_fundamentals WHERE symbol = ?1",
                )?;
                let row = stmt
                    .query_row(rusqlite::params![key], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    })
                    .ok();
                Ok(row)
            })
            .await?
            .map(
                |(symbol, as_of_date, source, payload_json, written_at, written_by)| {
                    let data: FundamentalData =
                        serde_json::from_str(&payload_json).map_err(StorageError::from)?;
                    Ok(ManualFundamentalsRow {
                        symbol,
                        as_of_date,
                        source,
                        data,
                        written_at,
                        written_by,
                    })
                },
            )
            .transpose()
    }

    /// Insert-or-replace the row for `symbol`. Returns the prior row (if
    /// any) alongside the new row so the caller can build a diff response.
    pub async fn upsert(
        &self,
        symbol: &str,
        data: FundamentalData,
        as_of_date: &str,
        source: &str,
        written_by: &str,
        written_at: i64,
    ) -> Result<UpsertOutcome, StorageError> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Err(StorageError::Migration(
                "manual_fundamentals symbol must be non-empty".to_string(),
            ));
        }
        let prior = self.get(&key).await?;
        let payload_json = serde_json::to_string(&data).map_err(StorageError::from)?;
        let key_for_write = key.clone();
        let as_of_owned = as_of_date.to_string();
        let source_owned = source.to_string();
        let written_by_owned = written_by.to_string();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO manual_fundamentals \
                       (symbol, as_of_date, source, payload_json, written_at, written_by) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                     ON CONFLICT(symbol) DO UPDATE SET \
                       as_of_date = excluded.as_of_date, \
                       source     = excluded.source, \
                       payload_json = excluded.payload_json, \
                       written_at = excluded.written_at, \
                       written_by = excluded.written_by",
                    rusqlite::params![
                        key_for_write,
                        as_of_owned,
                        source_owned,
                        payload_json,
                        written_at,
                        written_by_owned,
                    ],
                )?;
                Ok(())
            })
            .await?;
        let current = ManualFundamentalsRow {
            symbol: key,
            as_of_date: as_of_date.to_string(),
            source: source.to_string(),
            data,
            written_at,
            written_by: written_by.to_string(),
        };
        Ok(UpsertOutcome { current, prior })
    }

    /// Remove the row for `symbol`. No-op when the row is absent.
    pub async fn clear(&self, symbol: &str) -> Result<(), StorageError> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Ok(());
        }
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "DELETE FROM manual_fundamentals WHERE symbol = ?1",
                    rusqlite::params![key],
                )?;
                Ok(())
            })
            .await
    }

    /// Lightweight summary row used by the analysis UI banner. Returns
    /// `(symbol, as_of_date, written_at)` triples newest-first so the
    /// dashboard can flag stale entries.
    pub async fn list_with_freshness(&self) -> Result<Vec<(String, String, i64)>, StorageError> {
        self.db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT symbol, as_of_date, written_at \
                     FROM manual_fundamentals ORDER BY written_at DESC",
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }
}
