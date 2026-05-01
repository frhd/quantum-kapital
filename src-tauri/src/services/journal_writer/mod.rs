//! Phase 7 — journal_writer.
//!
//! Backs the `append_journal_entry` MCP write tool. Persists agent-
//! authored journal sections in SQLite (`journal_entries` table)
//! keyed by `(journal_date, section)` so re-runs upsert cleanly and
//! the user's manual notes are never clobbered.
//!
//! The render path is the `daily-journal` skill, which reads from
//! `journal_entries` and stitches the agent-authored sections into
//! `journal/YYYY-MM-DD.md`. Putting the data in SQLite (rather than
//! letting the MCP server poke at the filesystem) keeps the write
//! path race-free and means the MCP server doesn't need to know
//! where the journal directory lives.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

#[cfg(test)]
mod tests;

/// One row of `journal_entries`. `written_by` carries the caller
/// identity (`"agent_eod_review"`, `"interactive"`, …) so the journal
/// renderer can attribute sections inline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: i64,
    pub journal_date: NaiveDate,
    pub section: String,
    pub body_md: String,
    pub written_by: String,
    pub written_at: DateTime<Utc>,
}

/// Inputs for [`upsert_entry`]. Section is trimmed; both date and
/// section are part of the unique key.
#[derive(Debug, Clone)]
pub struct NewJournalEntry {
    pub journal_date: NaiveDate,
    pub section: String,
    pub body_md: String,
    pub written_by: String,
}

#[derive(Debug, Error)]
pub enum JournalWriterError {
    #[error("section must be non-empty")]
    EmptySection,
    #[error("body_md must be non-empty")]
    EmptyBody,
    #[error("written_by must be non-empty")]
    EmptyWrittenBy,
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
}

/// Upsert one entry on `(journal_date, section)`. A second call with
/// the same key overwrites the body — append-only-by-section
/// semantics from the Phase 7 plan.
pub async fn upsert_entry(
    db: &Arc<Db>,
    new: NewJournalEntry,
) -> Result<JournalEntry, JournalWriterError> {
    let section_trimmed = new.section.trim().to_string();
    if section_trimmed.is_empty() {
        return Err(JournalWriterError::EmptySection);
    }
    if new.body_md.trim().is_empty() {
        return Err(JournalWriterError::EmptyBody);
    }
    if new.written_by.trim().is_empty() {
        return Err(JournalWriterError::EmptyWrittenBy);
    }

    let now_unix = Utc::now().timestamp();
    let date_str = new.journal_date.to_string();
    let date_for_db = date_str.clone();
    let section_for_db = section_trimmed.clone();
    let body_for_db = new.body_md.clone();
    let written_by_for_db = new.written_by.clone();

    let id = db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT INTO journal_entries \
                 (journal_date, section, body_md, written_by, written_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(journal_date, section) DO UPDATE SET \
                     body_md = excluded.body_md, \
                     written_by = excluded.written_by, \
                     written_at = excluded.written_at",
                rusqlite::params![
                    date_for_db,
                    section_for_db,
                    body_for_db,
                    written_by_for_db,
                    now_unix
                ],
            )?;
            let id: i64 = conn.query_row(
                "SELECT id FROM journal_entries \
                 WHERE journal_date = ?1 AND section = ?2",
                rusqlite::params![date_str, section_trimmed],
                |row| row.get(0),
            )?;
            Ok(id)
        })
        .await?;

    Ok(JournalEntry {
        id,
        journal_date: new.journal_date,
        section: new.section,
        body_md: new.body_md,
        written_by: new.written_by,
        written_at: unix_to_utc(now_unix),
    })
}

/// Read every entry for `journal_date`, oldest-first by `written_at`
/// then `id` so the daily-journal renderer can render in stable order.
/// Currently exercised by the unit tests; the daily-journal skill
/// reads `journal_entries` directly via the SQLite CLI so the
/// production callsite is the skill, not Rust.
#[allow(dead_code)]
pub async fn list_entries_for_date(
    db: &Arc<Db>,
    journal_date: NaiveDate,
) -> Result<Vec<JournalEntry>, JournalWriterError> {
    let date_str = journal_date.to_string();
    let raws = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, journal_date, section, body_md, written_by, written_at \
                 FROM journal_entries WHERE journal_date = ?1 \
                 ORDER BY written_at ASC, id ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![date_str], row_to_raw)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;
    raws.into_iter().map(decode_raw).collect()
}

/// Look up a single section. Returns `Ok(None)` when absent.
#[allow(dead_code)] // exercised by tests; reserved for future render path.
pub async fn get_entry(
    db: &Arc<Db>,
    journal_date: NaiveDate,
    section: &str,
) -> Result<Option<JournalEntry>, JournalWriterError> {
    let date_str = journal_date.to_string();
    let section_owned = section.trim().to_string();
    let raw = db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT id, journal_date, section, body_md, written_by, written_at \
                 FROM journal_entries WHERE journal_date = ?1 AND section = ?2",
                rusqlite::params![date_str, section_owned],
                row_to_raw,
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;
    raw.map(decode_raw).transpose()
}

type RawRow = (i64, String, String, String, String, i64);

fn row_to_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn decode_raw(r: RawRow) -> Result<JournalEntry, JournalWriterError> {
    let (id, date_s, section, body_md, written_by, written_at) = r;
    let journal_date = NaiveDate::parse_from_str(&date_s, "%Y-%m-%d").map_err(|e| {
        JournalWriterError::Storage(StorageError::Migration(format!(
            "invalid journal_date '{date_s}': {e}"
        )))
    })?;
    Ok(JournalEntry {
        id,
        journal_date,
        section,
        body_md,
        written_by,
        written_at: unix_to_utc(written_at),
    })
}
