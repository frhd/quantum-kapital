//! Phase 04 — Tracker watchlist persistence.
//!
//! `TrackerService` is a thin CRUD layer on top of the `tracked_tickers`
//! table. It does **not** enforce status transitions (Phase 12 owns the
//! state machine); it stores whatever caller passes in. Tags and
//! `source_meta` round-trip through JSON columns so frontend payloads can
//! remain typed without schema migrations.

use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use rusqlite::OptionalExtension;
use thiserror::Error;

use crate::ibkr::types::tracker::{
    Setup, SetupStatus, StrategyTag, TrackedTicker, TrackerSource, TrackerStatus,
};
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::strategies::{Direction, SetupCandidate, TargetLevel};

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
                            "SELECT symbol, source, source_meta, status, tags, notes, added_at, last_checked_at, in_play_until, cool_down_until \
                             FROM tracked_tickers WHERE status = ?1 ORDER BY added_at DESC",
                        )?;
                        stmt.query_map(rusqlite::params![s], row_to_raw)?
                            .collect::<rusqlite::Result<Vec<_>>>()?
                    }
                    None => {
                        stmt = conn.prepare(
                            "SELECT symbol, source, source_meta, status, tags, notes, added_at, last_checked_at, in_play_until, cool_down_until \
                             FROM tracked_tickers ORDER BY added_at DESC",
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
                    "SELECT symbol, source, source_meta, status, tags, notes, added_at, last_checked_at, in_play_until, cool_down_until \
                     FROM tracked_tickers WHERE symbol = ?1",
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
                    "UPDATE tracked_tickers SET tags = ?1 WHERE symbol = ?2",
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
                    "UPDATE tracked_tickers SET status = ?1, in_play_until = ?2, cool_down_until = ?3 WHERE symbol = ?4",
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

    /// Count `Active` setups for `symbol`. Used by the state machine to
    /// decide whether invalidating one setup should flip the ticker to
    /// `CoolDown` (only when no other active setups remain).
    #[allow(dead_code)]
    pub async fn count_active_setups(&self, symbol: &str) -> Result<usize> {
        let symbol = symbol.to_uppercase();
        let count = self
            .db
            .with_conn(move |conn| {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM setups WHERE symbol = ?1 AND status = 'active'",
                    rusqlite::params![symbol],
                    |row| row.get(0),
                )?;
                Ok(n)
            })
            .await?;
        Ok(count.max(0) as usize)
    }

    /// Update a `setups` row's lifecycle status. When transitioning to
    /// `Invalidated`, callers pass the reason and the timestamp; for
    /// `Completed`, only the timestamp. Returns the persisted row, or
    /// `NotFound` if the id doesn't exist.
    #[allow(dead_code)]
    pub async fn update_setup_status(
        &self,
        id: i64,
        status: SetupStatus,
        reason: Option<String>,
        invalidated_at: Option<DateTime<Utc>>,
    ) -> Result<Setup> {
        let status_str = status.as_str().to_string();
        let reason_for_db = reason.clone();
        let invalidated_unix = invalidated_at.map(|d| d.timestamp());
        let updated = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE setups SET status = ?1, invalidation_reason = ?2, invalidated_at = ?3 WHERE id = ?4",
                    rusqlite::params![status_str, reason_for_db, invalidated_unix, id],
                )?;
                Ok(n)
            })
            .await?;
        if updated == 0 {
            return Err(TrackerError::NotFound(format!("setup#{id}")));
        }
        self.get_setup(id)
            .await?
            .ok_or_else(|| TrackerError::NotFound(format!("setup#{id}")))
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
                    "UPDATE tracked_tickers SET last_checked_at = ?1 WHERE symbol = ?2",
                    rusqlite::params![now_unix, symbol],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    // ---------------- setup CRUD (Phase 10) ----------------

    /// Insert a new `Setup` row from a detector candidate. The caller
    /// passes the symbol explicitly because `SetupCandidate` does not
    /// carry it (the detector frame already knows the symbol from the
    /// surrounding `MarketContext`). Returns the persisted row with its
    /// generated `id` populated.
    pub async fn insert_setup(&self, symbol: &str, candidate: &SetupCandidate) -> Result<Setup> {
        let symbol_norm = symbol.to_uppercase();
        let strategy = candidate.strategy.to_string();
        let direction = candidate.direction;
        let direction_str = direction_as_str(direction).to_string();
        let detected_at = candidate.detected_at;
        let detected_at_unix = detected_at.timestamp();
        let trigger_price = candidate.trigger_price;
        let stop_price = candidate.stop_price;
        let targets = candidate.targets.clone();
        let targets_json = serde_json::to_string(&targets)?;
        let raw_signals = candidate.raw_signals.clone();
        let raw_signals_json = serde_json::to_string(&raw_signals)?;

        let symbol_for_db = symbol_norm.clone();
        let strategy_for_db = strategy.clone();

        let id = self
            .db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO setups \
                     (symbol, strategy, direction, detected_at, trigger_price, stop_price, \
                      targets, raw_signals, thesis, thesis_json, status, invalidated_at, invalidation_reason) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, 'active', NULL, NULL)",
                    rusqlite::params![
                        symbol_for_db,
                        strategy_for_db,
                        direction_str,
                        detected_at_unix,
                        trigger_price,
                        stop_price,
                        targets_json,
                        raw_signals_json,
                    ],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await?;

        Ok(Setup {
            id,
            symbol: symbol_norm,
            strategy,
            direction,
            detected_at,
            trigger_price,
            stop_price,
            targets,
            raw_signals,
            thesis: None,
            thesis_json: None,
            status: SetupStatus::Active,
            invalidated_at: None,
            invalidation_reason: None,
        })
    }

    /// Persist a generated LLM thesis on a `setups` row. `thesis_md` is
    /// the human-readable markdown body; `thesis_json` is the full
    /// structured tool-output (conviction, invalidation_levels,
    /// risk_notes, …) stored as JSON. Returns the refreshed row, or
    /// `NotFound` if the id doesn't exist.
    pub async fn update_setup_thesis(
        &self,
        id: i64,
        thesis_md: String,
        thesis_json: serde_json::Value,
    ) -> Result<Setup> {
        let thesis_json_str = serde_json::to_string(&thesis_json)?;
        let updated = self
            .db
            .with_conn(move |conn| {
                let n = conn.execute(
                    "UPDATE setups SET thesis = ?1, thesis_json = ?2 WHERE id = ?3",
                    rusqlite::params![thesis_md, thesis_json_str, id],
                )?;
                Ok(n)
            })
            .await?;
        if updated == 0 {
            return Err(TrackerError::NotFound(format!("setup#{id}")));
        }
        self.get_setup(id)
            .await?
            .ok_or_else(|| TrackerError::NotFound(format!("setup#{id}")))
    }

    /// List setups, optionally filtered by `symbol` and / or by a
    /// `since` cutoff (rows with `detected_at >= since` only). Order is
    /// `detected_at DESC` so the freshest rows come first.
    pub async fn list_setups(
        &self,
        symbol: Option<&str>,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Setup>> {
        let symbol = symbol.map(|s| s.to_uppercase());
        let since_unix = since.map(|d| d.timestamp());
        let raws = self
            .db
            .with_conn(move |conn| {
                let mut sql = String::from(
                    "SELECT id, symbol, strategy, direction, detected_at, trigger_price, \
                            stop_price, targets, raw_signals, thesis, thesis_json, status, \
                            invalidated_at, invalidation_reason \
                     FROM setups",
                );
                let mut clauses: Vec<&'static str> = Vec::new();
                if symbol.is_some() {
                    clauses.push("symbol = ?1");
                }
                if since_unix.is_some() {
                    if symbol.is_some() {
                        clauses.push("detected_at >= ?2");
                    } else {
                        clauses.push("detected_at >= ?1");
                    }
                }
                if !clauses.is_empty() {
                    sql.push_str(" WHERE ");
                    sql.push_str(&clauses.join(" AND "));
                }
                sql.push_str(" ORDER BY detected_at DESC, id DESC");

                let mut stmt = conn.prepare(&sql)?;
                let rows = match (symbol.as_ref(), since_unix) {
                    (Some(s), Some(u)) => stmt
                        .query_map(rusqlite::params![s, u], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                    (Some(s), None) => stmt
                        .query_map(rusqlite::params![s], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                    (None, Some(u)) => stmt
                        .query_map(rusqlite::params![u], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                    (None, None) => stmt
                        .query_map([], setup_row_to_raw)?
                        .collect::<rusqlite::Result<Vec<_>>>()?,
                };
                Ok(rows)
            })
            .await?;

        raws.into_iter().map(decode_setup_raw).collect()
    }

    #[allow(dead_code)]
    pub async fn get_setup(&self, id: i64) -> Result<Option<Setup>> {
        let raw = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id, symbol, strategy, direction, detected_at, trigger_price, \
                            stop_price, targets, raw_signals, thesis, thesis_json, status, \
                            invalidated_at, invalidation_reason \
                     FROM setups WHERE id = ?1",
                    rusqlite::params![id],
                    setup_row_to_raw,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        match raw {
            Some(r) => Ok(Some(decode_setup_raw(r)?)),
            None => Ok(None),
        }
    }

    /// Look up the most recent `setups` row for `(symbol, strategy,
    /// direction)` whose `detected_at` falls within the last
    /// `within` window. Returns the row's `id` if a match exists, or
    /// `None` if there is no recent duplicate. Used by the runner to
    /// short-circuit re-emitting the same signal twice in a single
    /// trading day.
    pub async fn recent_duplicate(
        &self,
        symbol: &str,
        strategy: &str,
        direction: Direction,
        within: ChronoDuration,
    ) -> Result<Option<i64>> {
        let symbol = symbol.to_uppercase();
        let strategy = strategy.to_string();
        let direction_str = direction_as_str(direction).to_string();
        let cutoff_unix = (Utc::now() - within).timestamp();
        let id = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id FROM setups \
                     WHERE symbol = ?1 AND strategy = ?2 AND direction = ?3 \
                       AND detected_at >= ?4 \
                     ORDER BY detected_at DESC LIMIT 1",
                    rusqlite::params![symbol, strategy, direction_str, cutoff_unix],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        Ok(id)
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

fn decode_raw(r: RawRow) -> Result<TrackedTicker> {
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
    })
}

fn unix_to_utc(ts: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now)
}

// ---------------- setup row helpers (Phase 10) ----------------

type SetupRawRow = (
    i64,            // id
    String,         // symbol
    String,         // strategy
    String,         // direction
    i64,            // detected_at unix
    f64,            // trigger_price
    f64,            // stop_price
    String,         // targets json
    String,         // raw_signals json
    Option<String>, // thesis (markdown)
    Option<String>, // thesis_json (full structured)
    String,         // status
    Option<i64>,    // invalidated_at unix
    Option<String>, // invalidation_reason
);

fn setup_row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<SetupRawRow> {
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

fn decode_setup_raw(r: SetupRawRow) -> Result<Setup> {
    let (
        id,
        symbol,
        strategy,
        direction_s,
        detected_at,
        trigger_price,
        stop_price,
        targets_s,
        raw_signals_s,
        thesis,
        thesis_json_s,
        status_s,
        invalidated_at,
        invalidation_reason,
    ) = r;
    let direction = parse_direction(&direction_s).ok_or_else(|| {
        TrackerError::Storage(StorageError::Migration(format!(
            "unknown direction '{direction_s}' on setup {id}"
        )))
    })?;
    let status = SetupStatus::parse(&status_s).ok_or_else(|| {
        TrackerError::Storage(StorageError::Migration(format!(
            "unknown setup status '{status_s}' on setup {id}"
        )))
    })?;
    let targets: Vec<TargetLevel> = serde_json::from_str(&targets_s)?;
    let raw_signals: serde_json::Value = serde_json::from_str(&raw_signals_s)?;
    let thesis_json = match thesis_json_s {
        Some(s) if !s.is_empty() => Some(serde_json::from_str::<serde_json::Value>(&s)?),
        _ => None,
    };
    Ok(Setup {
        id,
        symbol,
        strategy,
        direction,
        detected_at: unix_to_utc(detected_at),
        trigger_price,
        stop_price,
        targets,
        raw_signals,
        thesis,
        thesis_json,
        status,
        invalidated_at: invalidated_at.map(unix_to_utc),
        invalidation_reason,
    })
}

fn direction_as_str(direction: Direction) -> &'static str {
    match direction {
        Direction::Long => "long",
        Direction::Short => "short",
    }
}

fn parse_direction(s: &str) -> Option<Direction> {
    match s {
        "long" => Some(Direction::Long),
        "short" => Some(Direction::Short),
        _ => None,
    }
}
