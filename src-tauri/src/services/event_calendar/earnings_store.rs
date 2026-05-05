//! SQLite-backed earnings-date stores.
//!
//! `EarningsOverridesStore` writes to `event_calendar_overrides` —
//! operator-curated next-earnings entries that always win over the AV
//! cache. Mirrors `manual_fundamentals` in spirit.
//!
//! `EarningsCacheStore` writes to `event_calendar_cache` — the
//! refresh-weekly memo of AV's `EARNINGS` response. Cache rows expire
//! after `CACHE_FRESHNESS_DAYS` (master plan: 7 days).

use std::sync::Arc;

use chrono::{NaiveDate, Utc};

use crate::storage::error::StorageError;
use crate::storage::Db;

use super::types::BlackoutConfidence;

/// Master plan: refresh weekly to keep AV daily quota intact.
pub const CACHE_FRESHNESS_DAYS: i64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarningsRow {
    pub symbol: String,
    pub next_earnings_date: NaiveDate,
    pub confidence: BlackoutConfidence,
    pub source: String,
    pub fetched_at_unix: i64,
}

/// Row in `event_calendar_overrides`. The `notes` column is a free-text
/// audit field (e.g. "trader pasted from issuer IR page 2026-05-06").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideRow {
    pub symbol: String,
    pub next_earnings_date: NaiveDate,
    pub confidence: BlackoutConfidence,
    pub written_at_unix: i64,
    pub written_by: String,
    pub notes: Option<String>,
}

#[derive(Clone)]
pub struct EarningsOverridesStore {
    db: Arc<Db>,
}

impl EarningsOverridesStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    pub async fn get(&self, symbol: &str) -> Result<Option<OverrideRow>, StorageError> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Ok(None);
        }
        let row = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT symbol, next_earnings_date, confidence, written_at, \
                            written_by, notes \
                     FROM event_calendar_overrides WHERE symbol = ?1",
                )?;
                let row = stmt
                    .query_row(rusqlite::params![key], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                        ))
                    })
                    .ok();
                Ok(row)
            })
            .await?;

        match row {
            Some((symbol, date_s, conf_s, written_at, written_by, notes)) => {
                let next_earnings_date = parse_date(&date_s)?;
                let confidence = parse_confidence(&conf_s)?;
                Ok(Some(OverrideRow {
                    symbol,
                    next_earnings_date,
                    confidence,
                    written_at_unix: written_at,
                    written_by,
                    notes,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn upsert(
        &self,
        symbol: &str,
        next_earnings_date: NaiveDate,
        confidence: BlackoutConfidence,
        written_by: &str,
        notes: Option<String>,
    ) -> Result<OverrideRow, StorageError> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Err(StorageError::Migration(
                "event_calendar_overrides symbol must be non-empty".to_string(),
            ));
        }
        let date_s = next_earnings_date.format("%Y-%m-%d").to_string();
        let conf_s = confidence.as_str().to_string();
        let written_at = Utc::now().timestamp();
        let written_by_owned = written_by.to_string();
        let notes_owned = notes.clone();
        let key_for_db = key.clone();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO event_calendar_overrides \
                       (symbol, next_earnings_date, confidence, written_at, \
                        written_by, notes) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                     ON CONFLICT(symbol) DO UPDATE SET \
                       next_earnings_date = excluded.next_earnings_date, \
                       confidence = excluded.confidence, \
                       written_at = excluded.written_at, \
                       written_by = excluded.written_by, \
                       notes = excluded.notes",
                    rusqlite::params![
                        key_for_db,
                        date_s,
                        conf_s,
                        written_at,
                        written_by_owned,
                        notes_owned,
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(OverrideRow {
            symbol: key,
            next_earnings_date,
            confidence,
            written_at_unix: written_at,
            written_by: written_by.to_string(),
            notes,
        })
    }

    pub async fn clear(&self, symbol: &str) -> Result<(), StorageError> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Ok(());
        }
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "DELETE FROM event_calendar_overrides WHERE symbol = ?1",
                    rusqlite::params![key],
                )?;
                Ok(())
            })
            .await
    }
}

#[derive(Clone)]
pub struct EarningsCacheStore {
    db: Arc<Db>,
}

impl EarningsCacheStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    pub async fn get(&self, symbol: &str) -> Result<Option<EarningsRow>, StorageError> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Ok(None);
        }
        let row = self
            .db
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT symbol, next_earnings_date, confidence, fetched_at, source \
                     FROM event_calendar_cache WHERE symbol = ?1",
                )?;
                let row = stmt
                    .query_row(rusqlite::params![key], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })
                    .ok();
                Ok(row)
            })
            .await?;

        match row {
            Some((symbol, date_s, conf_s, fetched_at, source)) => {
                let next_earnings_date = parse_date(&date_s)?;
                let confidence = parse_confidence(&conf_s)?;
                Ok(Some(EarningsRow {
                    symbol,
                    next_earnings_date,
                    confidence,
                    source,
                    fetched_at_unix: fetched_at,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn upsert(
        &self,
        symbol: &str,
        next_earnings_date: NaiveDate,
        confidence: BlackoutConfidence,
        source: &str,
    ) -> Result<EarningsRow, StorageError> {
        let key = symbol.trim().to_uppercase();
        if key.is_empty() {
            return Err(StorageError::Migration(
                "event_calendar_cache symbol must be non-empty".to_string(),
            ));
        }
        let date_s = next_earnings_date.format("%Y-%m-%d").to_string();
        let conf_s = confidence.as_str().to_string();
        let fetched_at = Utc::now().timestamp();
        let source_owned = source.to_string();
        let key_for_db = key.clone();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO event_calendar_cache \
                       (symbol, next_earnings_date, confidence, fetched_at, source) \
                     VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT(symbol) DO UPDATE SET \
                       next_earnings_date = excluded.next_earnings_date, \
                       confidence = excluded.confidence, \
                       fetched_at = excluded.fetched_at, \
                       source = excluded.source",
                    rusqlite::params![key_for_db, date_s, conf_s, fetched_at, source_owned],
                )?;
                Ok(())
            })
            .await?;
        Ok(EarningsRow {
            symbol: key,
            next_earnings_date,
            confidence,
            source: source.to_string(),
            fetched_at_unix: fetched_at,
        })
    }

    /// Delete all cache rows. Used by `event_calendar_force_refresh` so
    /// the next lookup re-fetches from upstream.
    pub async fn clear_all(&self) -> Result<(), StorageError> {
        self.db
            .with_conn(move |conn| {
                conn.execute("DELETE FROM event_calendar_cache", [])?;
                Ok(())
            })
            .await
    }

    /// `true` when the row's `fetched_at` is within
    /// `CACHE_FRESHNESS_DAYS` of `now`. Stale rows are still readable
    /// (the gate prefers stale-cache to AV-fetch failure) but the
    /// composite calendar treats them as a tier below "fresh".
    pub fn is_fresh(row: &EarningsRow, now: chrono::DateTime<Utc>) -> bool {
        let age_secs = now.timestamp().saturating_sub(row.fetched_at_unix);
        age_secs < CACHE_FRESHNESS_DAYS * 86_400
    }
}

fn parse_date(s: &str) -> Result<NaiveDate, StorageError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
        StorageError::Migration(format!("event_calendar: bad date '{s}': {e}"))
    })
}

fn parse_confidence(s: &str) -> Result<BlackoutConfidence, StorageError> {
    BlackoutConfidence::parse(s).ok_or_else(|| {
        StorageError::Migration(format!("event_calendar: bad confidence '{s}'"))
    })
}
