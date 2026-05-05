//! `PlaybookStore` — writer + reader for the `playbooks` table.
//!
//! Writes append a new row per `(date, account)` with a server-assigned
//! `generation_id = MAX(generation_id) + 1`. v1 only writes one
//! generation per day; the schema already admits more so an intraday
//! refresh hook is a future no-migration change.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension};
use thiserror::Error;

use crate::storage::error::StorageError;
use crate::storage::Db;

use super::types::{Playbook, RankedSetup, SkipEntry, WritePlaybookRequest};

#[derive(Error, Debug)]
pub enum PlaybookError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid persisted row: {0}")]
    InvalidRow(String),
    #[error("account must be non-empty")]
    EmptyAccount,
}

/// Outcome of a write — exposes the assigned `generation_id` so the
/// MCP rail can echo it back to the caller.
#[derive(Debug, Clone)]
pub struct WriteOutcome {
    pub playbook: Playbook,
}

#[derive(Clone)]
pub struct PlaybookStore {
    db: Arc<Db>,
}

impl PlaybookStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Append a new playbook for `(date, account)`. The store assigns
    /// `generation_id = COALESCE(MAX(generation_id), 0) + 1` inside the
    /// same connection, so concurrent writers serialize through `Db`'s
    /// pool rather than racing.
    pub async fn write(&self, req: WritePlaybookRequest) -> Result<WriteOutcome, PlaybookError> {
        if req.account.trim().is_empty() {
            return Err(PlaybookError::EmptyAccount);
        }

        let now = Utc::now();
        let date_str = req.date.to_string();
        let account = req.account.clone();
        let generated_at_str = now.to_rfc3339();
        let setups_json = serde_json::to_string(&req.ranked_setups)?;
        let skip_json = serde_json::to_string(&req.skip_list)?;
        let llm_call_id = req.llm_call_id.clone();

        let setups_for_row = req.ranked_setups.clone();
        let skip_for_row = req.skip_list.clone();

        let date_for_row = req.date;
        let account_for_row = req.account.clone();
        let llm_for_row = req.llm_call_id.clone();

        let generation_id: i32 = self
            .db
            .with_conn(move |conn| {
                let next: i32 = conn.query_row(
                    "SELECT COALESCE(MAX(generation_id), 0) + 1 FROM playbooks
                     WHERE date = ?1 AND account = ?2",
                    params![date_str, account],
                    |r| r.get(0),
                )?;
                conn.execute(
                    "INSERT INTO playbooks (
                        date, account, generation_id, generated_at,
                        ranked_setups, skip_list, llm_call_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        date_str,
                        account,
                        next,
                        generated_at_str,
                        setups_json,
                        skip_json,
                        llm_call_id,
                    ],
                )?;
                Ok(next)
            })
            .await?;

        Ok(WriteOutcome {
            playbook: Playbook {
                date: date_for_row,
                account: account_for_row,
                generation_id,
                generated_at: now,
                ranked_setups: setups_for_row,
                skip_list: skip_for_row,
                llm_call_id: llm_for_row,
            },
        })
    }

    /// Read the latest generation for `(date, account)`. Returns
    /// `Ok(None)` when no row exists.
    pub async fn read_latest(
        &self,
        date: NaiveDate,
        account: &str,
    ) -> Result<Option<Playbook>, PlaybookError> {
        let date_str = date.to_string();
        let account = account.to_string();
        let row: Option<RawRow> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT date, account, generation_id, generated_at,
                            ranked_setups, skip_list, llm_call_id
                     FROM playbooks
                     WHERE date = ?1 AND account = ?2
                     ORDER BY generation_id DESC
                     LIMIT 1",
                    params![date_str, account],
                    map_raw,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        row.map(parse_row).transpose()
    }

    /// Read a specific `generation_id` for `(date, account)`. Returns
    /// `Ok(None)` when no row exists.
    pub async fn read_generation(
        &self,
        date: NaiveDate,
        account: &str,
        generation_id: i32,
    ) -> Result<Option<Playbook>, PlaybookError> {
        let date_str = date.to_string();
        let account = account.to_string();
        let row: Option<RawRow> = self
            .db
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT date, account, generation_id, generated_at,
                            ranked_setups, skip_list, llm_call_id
                     FROM playbooks
                     WHERE date = ?1 AND account = ?2 AND generation_id = ?3",
                    params![date_str, account, generation_id],
                    map_raw,
                )
                .optional()
                .map_err(StorageError::from)
            })
            .await?;
        row.map(parse_row).transpose()
    }

    /// Count rows in the table — observability hook for tests.
    #[cfg(test)]
    pub async fn count(&self) -> Result<i64, PlaybookError> {
        let n: i64 = self
            .db
            .with_conn(|conn| {
                let n: i64 = conn.query_row("SELECT COUNT(*) FROM playbooks", [], |r| r.get(0))?;
                Ok(n)
            })
            .await?;
        Ok(n)
    }
}

struct RawRow {
    date: String,
    account: String,
    generation_id: i32,
    generated_at: String,
    ranked_setups: String,
    skip_list: String,
    llm_call_id: Option<String>,
}

fn map_raw(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok(RawRow {
        date: row.get(0)?,
        account: row.get(1)?,
        generation_id: row.get(2)?,
        generated_at: row.get(3)?,
        ranked_setups: row.get(4)?,
        skip_list: row.get(5)?,
        llm_call_id: row.get(6)?,
    })
}

fn parse_row(raw: RawRow) -> Result<Playbook, PlaybookError> {
    let date = NaiveDate::parse_from_str(&raw.date, "%Y-%m-%d")
        .map_err(|e| PlaybookError::InvalidRow(format!("date `{}`: {e}", raw.date)))?;
    let generated_at = DateTime::parse_from_rfc3339(&raw.generated_at)
        .map_err(|e| {
            PlaybookError::InvalidRow(format!("generated_at `{}`: {e}", raw.generated_at))
        })?
        .with_timezone(&Utc);
    let ranked_setups: Vec<RankedSetup> = serde_json::from_str(&raw.ranked_setups)?;
    let skip_list: Vec<SkipEntry> = serde_json::from_str(&raw.skip_list)?;
    Ok(Playbook {
        date,
        account: raw.account,
        generation_id: raw.generation_id,
        generated_at,
        ranked_setups,
        skip_list,
        llm_call_id: raw.llm_call_id,
    })
}
