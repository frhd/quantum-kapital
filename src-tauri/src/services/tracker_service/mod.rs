//! Phase 04 — Tracker watchlist persistence.
//!
//! `TrackerService` is a thin CRUD layer on top of the `tracked_tickers`
//! table. It does **not** enforce status transitions (Phase 12 owns the
//! state machine); it stores whatever caller passes in. Tags and
//! `source_meta` round-trip through JSON columns so frontend payloads can
//! remain typed without schema migrations.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use thiserror::Error;

use crate::ibkr::types::tracker::{StrategyTag, TrackedTicker, TrackerSource, TrackerStatus};
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

mod archive;
mod setups;

#[cfg(test)]
mod tests;

#[derive(Error, Debug)]
pub enum TrackerError {
    #[error("ticker '{0}' is already tracked")]
    AlreadyTracked(String),
    #[error("ticker '{0}' is not tracked")]
    NotFound(String),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, TrackerError>;

#[derive(Clone)]
pub struct TrackerService {
    db: Arc<Db>,
}

impl TrackerService {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Test-only accessor for the shared `Db` handle. Phase 21 alert-
    /// wiring tests need to read the `alerts` table through
    /// `services::alerts::list_alerts`, which takes `&Arc<Db>`.
    #[cfg(test)]
    pub fn db_for_testing(&self) -> Arc<Db> {
        Arc::clone(&self.db)
    }

    pub async fn add(
        &self,
        symbol: &str,
        source: TrackerSource,
        source_meta: Option<serde_json::Value>,
        tags: Vec<StrategyTag>,
        notes: Option<String>,
    ) -> Result<TrackedTicker> {
        let symbol_norm = symbol.to_uppercase();
        let added_at = Utc::now();
        let row = TrackedTicker {
            symbol: symbol_norm.clone(),
            source,
            source_meta: source_meta.clone(),
            status: TrackerStatus::Watching,
            tags: tags.clone(),
            notes: notes.clone(),
            added_at,
            last_checked_at: None,
            in_play_until: None,
            cool_down_until: None,
            archived_at: None,
        };

        let source_str = source.as_str().to_string();
        let source_meta_json = source_meta
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let tags_json = serde_json::to_string(&tags)?;
        let added_at_unix = added_at.timestamp();
        let symbol_for_db = symbol_norm.clone();
        let notes_for_db = notes.clone();

        self.db
            .with_conn(move |conn| {
                let result = conn.execute(
                    "INSERT INTO tracked_tickers \
                     (symbol, source, source_meta, status, tags, notes, added_at, last_checked_at, in_play_until, cool_down_until) \
                     VALUES (?1, ?2, ?3, 'watching', ?4, ?5, ?6, NULL, NULL, NULL)",
                    rusqlite::params![
                        symbol_for_db,
                        source_str,
                        source_meta_json,
                        tags_json,
                        notes_for_db,
                        added_at_unix,
                    ],
                );
                match result {
                    Ok(_) => Ok(()),
                    Err(rusqlite::Error::SqliteFailure(err, _))
                        if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        Err(StorageError::Migration(String::from("__already_tracked__")))
                    }
                    Err(e) => Err(StorageError::Sqlite(e)),
                }
            })
            .await
            .map_err(|e| match e {
                StorageError::Migration(s) if s == "__already_tracked__" => {
                    TrackerError::AlreadyTracked(symbol_norm.clone())
                }
                other => TrackerError::Storage(other),
            })?;

        Ok(row)
    }

    pub async fn remove(&self, symbol: &str) -> Result<()> {
        let symbol = symbol.to_uppercase();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "DELETE FROM tracked_tickers WHERE symbol = ?1",
                    rusqlite::params![symbol],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn list(&self, status_filter: Option<TrackerStatus>) -> Result<Vec<TrackedTicker>> {
        let filter = status_filter.map(|s| s.as_str().to_string());
        let rows = self
            .db
            .with_conn(move |conn| {
                let mut stmt;
                let iter = match &filter {
                    Some(s) => {
                        stmt = conn.prepare(
                            "SELECT symbol, source, source_meta, status, tags, notes, added_at, last_checked_at, in_play_until, cool_down_until, archived_at \
                             FROM tracked_tickers WHERE status = ?1 AND archived_at IS NULL ORDER BY added_at DESC",
                        )?;
                        stmt.query_map(rusqlite::params![s], row_to_raw)?
                            .collect::<rusqlite::Result<Vec<_>>>()?
                    }
                    None => {
                        stmt = conn.prepare(
                            "SELECT symbol, source, source_meta, status, tags, notes, added_at, last_checked_at, in_play_until, cool_down_until, archived_at \
                             FROM tracked_tickers WHERE archived_at IS NULL ORDER BY added_at DESC",
                        )?;
                        stmt.query_map([], row_to_raw)?
                            .collect::<rusqlite::Result<Vec<_>>>()?
                    }
                };
                Ok(iter)
            })
            .await?;

        rows.into_iter().map(decode_raw).collect::<Result<Vec<_>>>()
    }

    pub async fn get(&self, symbol: &str) -> Result<Option<TrackedTicker>> {
        let symbol = symbol.to_uppercase();
        let raw = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT symbol, source, source_meta, status, tags, notes, added_at, last_checked_at, in_play_until, cool_down_until, archived_at \
                     FROM tracked_tickers WHERE symbol = ?1 AND archived_at IS NULL",
                    rusqlite::params![symbol],
                    row_to_raw,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        match raw {
            Some(r) => Ok(Some(decode_raw(r)?)),
            None => Ok(None),
        }
    }

    pub async fn set_tags(&self, symbol: &str, tags: Vec<StrategyTag>) -> Result<TrackedTicker> {
        let symbol_norm = symbol.to_uppercase();
        let tags_json = serde_json::to_string(&tags)?;
        let symbol_for_db = symbol_norm.clone();
        let updated = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE tracked_tickers SET tags = ?1 \
                     WHERE symbol = ?2 AND archived_at IS NULL",
                    rusqlite::params![tags_json, symbol_for_db],
                )?;
                Ok(n)
            })
            .await?;
        if updated == 0 {
            return Err(TrackerError::NotFound(symbol_norm));
        }
        // Re-read to return canonical persisted state.
        self.get(&symbol_norm)
            .await?
            .ok_or(TrackerError::NotFound(symbol_norm))
    }

    pub async fn set_status(
        &self,
        symbol: &str,
        status: TrackerStatus,
        in_play_until: Option<DateTime<Utc>>,
        cool_down_until: Option<DateTime<Utc>>,
    ) -> Result<TrackedTicker> {
        let symbol_norm = symbol.to_uppercase();
        let status_str = status.as_str().to_string();
        let in_play_unix = in_play_until.map(|d| d.timestamp());
        let cool_down_unix = cool_down_until.map(|d| d.timestamp());
        let symbol_for_db = symbol_norm.clone();
        let updated = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE tracked_tickers SET status = ?1, in_play_until = ?2, cool_down_until = ?3 \
                     WHERE symbol = ?4 AND archived_at IS NULL",
                    rusqlite::params![status_str, in_play_unix, cool_down_unix, symbol_for_db],
                )?;
                Ok(n)
            })
            .await?;
        if updated == 0 {
            return Err(TrackerError::NotFound(symbol_norm));
        }
        self.get(&symbol_norm)
            .await?
            .ok_or(TrackerError::NotFound(symbol_norm))
    }

    /// Stamp `last_checked_at = now`. Phase 13/14 schedulers will call
    /// this; Phase 04 only exercises it via tests.
    #[allow(dead_code)]
    pub async fn touch_last_checked(&self, symbol: &str) -> Result<()> {
        let symbol = symbol.to_uppercase();
        let now_unix = Utc::now().timestamp();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "UPDATE tracked_tickers SET last_checked_at = ?1 \
                     WHERE symbol = ?2 AND archived_at IS NULL",
                    rusqlite::params![now_unix, symbol],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}

// ---------------- internals ----------------

/// Tuple of column values as read from `tracked_tickers`. We decode into
/// strongly-typed [`TrackedTicker`] in a second step so JSON parse errors
/// surface as `TrackerError::Serde` rather than `rusqlite::Error`.
type RawRow = (
    String,         // symbol
    String,         // source
    Option<String>, // source_meta json
    String,         // status
    String,         // tags json
    Option<String>, // notes
    i64,            // added_at unix
    Option<i64>,    // last_checked_at unix
    Option<i64>,    // in_play_until unix
    Option<i64>,    // cool_down_until unix
    Option<i64>,    // archived_at unix
);

pub(super) fn row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
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
    ))
}

pub(super) fn decode_raw(r: RawRow) -> Result<TrackedTicker> {
    let (
        symbol,
        source_s,
        source_meta_s,
        status_s,
        tags_s,
        notes,
        added_at,
        last_checked,
        in_play,
        cool_down,
        archived,
    ) = r;
    let source = TrackerSource::parse(&source_s).ok_or_else(|| {
        TrackerError::Storage(StorageError::Migration(format!(
            "unknown tracker source '{source_s}' for {symbol}"
        )))
    })?;
    let status = TrackerStatus::parse(&status_s).ok_or_else(|| {
        TrackerError::Storage(StorageError::Migration(format!(
            "unknown tracker status '{status_s}' for {symbol}"
        )))
    })?;
    let source_meta = match source_meta_s {
        Some(s) if !s.is_empty() => Some(serde_json::from_str::<serde_json::Value>(&s)?),
        _ => None,
    };
    let tags: Vec<StrategyTag> = serde_json::from_str(&tags_s)?;
    Ok(TrackedTicker {
        symbol,
        source,
        source_meta,
        status,
        tags,
        notes,
        added_at: unix_to_utc(added_at),
        last_checked_at: last_checked.map(unix_to_utc),
        in_play_until: in_play.map(unix_to_utc),
        cool_down_until: cool_down.map(unix_to_utc),
        archived_at: archived.map(unix_to_utc),
    })
}
