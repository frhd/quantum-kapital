//! Phase 02 — Research notes durable artifacts.
//!
//! `research_notes` rows are the LLM-authored output of the headless agent
//! (morning sweep / per-alert dive) and the interactive Claude Code
//! sessions. Every row carries explicit provenance via `written_by` so the
//! UI / eval harness can tell agent-authored notes apart from human
//! ones.
//!
//! The module is intentionally thin: free functions over `&Arc<Db>`,
//! mirroring `mcp_audit`. There is no in-memory state; tests open the
//! same `Db` the live handler uses. `EvidenceRef` is the **closed** set
//! of pointer types research notes may reference (`alert`, `news`,
//! `setup`, `bar_range`) — locked early so the schema doesn't sprawl.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

#[cfg(test)]
mod tests;

/// Closed-by-design set of pointer types a research note may attach.
///
/// Adding a variant here is intentionally a code change and a code review,
/// not an open-ended JSON schema — the master plan calls out
/// `evidence_refs` schema sprawl as a Phase-2 gotcha. Variants serialize
/// with a `type` discriminator (see `#[serde(tag = "type")]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvidenceRef {
    /// Reference to an `alerts.id` row.
    Alert { id: i64 },
    /// Reference to a `news_cache.id` row (the cached article).
    News { cache_id: i64 },
    /// Reference to a `setups.id` row.
    Setup { id: i64 },
    /// A windowed slice of bars for `symbol` between two RFC3339 instants.
    /// Lets a note point to the chart segment that supports its claim
    /// without copying bar data into the note body.
    BarRange {
        symbol: String,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
}

/// Conviction grade. Locked at A/B/C until the eval harness justifies a
/// finer taxonomy (master plan Phase-2 gotcha #2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Conviction {
    A,
    B,
    C,
}

impl Conviction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Conviction::A => "A",
            Conviction::B => "B",
            Conviction::C => "C",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "A" => Some(Conviction::A),
            "B" => Some(Conviction::B),
            "C" => Some(Conviction::C),
            _ => None,
        }
    }
}

/// One row of `research_notes`. `written_by` carries the caller
/// identifier (`"user"`, `"agent_morning_sweep"`, `"agent_alert_dive"`,
/// `"interactive"`, …); the agent loops set this to their loop name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchNote {
    pub id: i64,
    pub symbol: String,
    pub body_md: String,
    pub conviction: Option<Conviction>,
    pub evidence_refs: Vec<EvidenceRef>,
    pub written_by: String,
    pub written_at: DateTime<Utc>,
    pub setup_id: Option<i64>,
    pub alert_id: Option<i64>,
}

/// Inputs for [`write_note`]. Symbol is normalized to upper-case before
/// insert so reads can match without case-folding.
#[derive(Debug, Clone)]
pub struct NewResearchNote {
    pub symbol: String,
    pub body_md: String,
    pub conviction: Option<Conviction>,
    pub evidence_refs: Vec<EvidenceRef>,
    pub written_by: String,
    pub setup_id: Option<i64>,
    pub alert_id: Option<i64>,
}

#[derive(Error, Debug)]
pub enum ResearchNotesError {
    #[error("symbol must be non-empty")]
    EmptySymbol,
    #[error("body_md must be non-empty")]
    EmptyBody,
    #[error("written_by must be non-empty")]
    EmptyWrittenBy,
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Persist a new research note. Returns the populated row (with its
/// generated `id`) so callers can reference it from `mcp_audit.result_summary`.
pub async fn write_note(
    db: &Arc<Db>,
    new: NewResearchNote,
) -> Result<ResearchNote, ResearchNotesError> {
    let symbol = new.symbol.trim();
    if symbol.is_empty() {
        return Err(ResearchNotesError::EmptySymbol);
    }
    if new.body_md.trim().is_empty() {
        return Err(ResearchNotesError::EmptyBody);
    }
    if new.written_by.trim().is_empty() {
        return Err(ResearchNotesError::EmptyWrittenBy);
    }

    let symbol_norm = symbol.to_uppercase();
    let conviction_str = new.conviction.map(|c| c.as_str().to_string());
    let evidence_json = serde_json::to_string(&new.evidence_refs)?;
    // Truncate to whole seconds so the in-memory timestamp matches what
    // `get_note` will round-trip after reading the unix-second column.
    let now_unix = Utc::now().timestamp();
    let now = unix_to_utc(now_unix);
    let symbol_for_db = symbol_norm.clone();
    let body_for_db = new.body_md.clone();
    let written_by_for_db = new.written_by.clone();
    let setup_id = new.setup_id;
    let alert_id = new.alert_id;

    let id = db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT INTO research_notes \
                 (symbol, body_md, conviction, evidence_refs, written_by, written_at, setup_id, alert_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    symbol_for_db,
                    body_for_db,
                    conviction_str,
                    evidence_json,
                    written_by_for_db,
                    now_unix,
                    setup_id,
                    alert_id,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .await?;

    Ok(ResearchNote {
        id,
        symbol: symbol_norm,
        body_md: new.body_md,
        conviction: new.conviction,
        evidence_refs: new.evidence_refs,
        written_by: new.written_by,
        written_at: now,
        setup_id,
        alert_id,
    })
}

/// Pagination + filter inputs for [`list_notes`].
#[derive(Debug, Clone, Default)]
pub struct ListNotesQuery {
    pub symbol: Option<String>,
    pub setup_id: Option<i64>,
    pub alert_id: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

/// Read research notes, newest-first. All filters AND-combine.
pub async fn list_notes(
    db: &Arc<Db>,
    query: ListNotesQuery,
) -> Result<Vec<ResearchNote>, ResearchNotesError> {
    let limit = query.limit.max(1) as i64;
    let offset = query.offset as i64;
    let symbol = query.symbol.map(|s| s.to_uppercase());
    let setup_id = query.setup_id;
    let alert_id = query.alert_id;

    let raws = db
        .with_conn(move |conn| {
            let mut sql = String::from(
                "SELECT id, symbol, body_md, conviction, evidence_refs, \
                        written_by, written_at, setup_id, alert_id \
                 FROM research_notes",
            );
            let mut clauses: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            if let Some(sym) = symbol {
                clauses.push(format!("symbol = ?{}", params.len() + 1));
                params.push(Box::new(sym));
            }
            if let Some(sid) = setup_id {
                clauses.push(format!("setup_id = ?{}", params.len() + 1));
                params.push(Box::new(sid));
            }
            if let Some(aid) = alert_id {
                clauses.push(format!("alert_id = ?{}", params.len() + 1));
                params.push(Box::new(aid));
            }
            if !clauses.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&clauses.join(" AND "));
            }
            sql.push_str(" ORDER BY written_at DESC, id DESC");
            sql.push_str(&format!(
                " LIMIT ?{} OFFSET ?{}",
                params.len() + 1,
                params.len() + 2
            ));
            params.push(Box::new(limit));
            params.push(Box::new(offset));

            let param_refs: Vec<&dyn rusqlite::ToSql> =
                params.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(param_refs.as_slice(), row_to_raw)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;

    raws.into_iter().map(decode_raw).collect()
}

/// Fetch a single note by id. Returns `Ok(None)` when the row is absent.
pub async fn get_note(
    db: &Arc<Db>,
    id: i64,
) -> Result<Option<ResearchNote>, ResearchNotesError> {
    let raw = db
        .with_conn(move |conn| {
            use rusqlite::OptionalExtension;
            conn.query_row(
                "SELECT id, symbol, body_md, conviction, evidence_refs, \
                        written_by, written_at, setup_id, alert_id \
                 FROM research_notes WHERE id = ?1",
                rusqlite::params![id],
                row_to_raw,
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;
    raw.map(decode_raw).transpose()
}

// ---------------- internals ----------------

type RawRow = (
    i64,            // id
    String,         // symbol
    String,         // body_md
    Option<String>, // conviction
    Option<String>, // evidence_refs json
    String,         // written_by
    i64,            // written_at unix
    Option<i64>,    // setup_id
    Option<i64>,    // alert_id
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
    ))
}

fn decode_raw(r: RawRow) -> Result<ResearchNote, ResearchNotesError> {
    let (id, symbol, body_md, conviction_s, evidence_s, written_by, written_at, setup_id, alert_id) =
        r;
    let conviction = conviction_s
        .as_deref()
        .and_then(Conviction::parse);
    let evidence_refs = match evidence_s {
        Some(s) if !s.is_empty() => serde_json::from_str(&s)?,
        _ => Vec::new(),
    };
    Ok(ResearchNote {
        id,
        symbol,
        body_md,
        conviction,
        evidence_refs,
        written_by,
        written_at: unix_to_utc(written_at),
        setup_id,
        alert_id,
    })
}
