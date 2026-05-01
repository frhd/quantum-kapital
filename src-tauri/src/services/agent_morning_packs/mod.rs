//! Phase 02 — Agent-authored morning packs (write surface).
//!
//! Distinct from `services::daily_ranker::MorningPack`, which the
//! deterministic EOD scheduler persists into the `morning_packs` table.
//! Agent loops emit ranked-idea packs into `agent_morning_packs` (one row
//! per `YYYY-MM-DD`, last write wins) so the source-of-output is clear in
//! analytics and the eval harness.
//!
//! Idempotency: `write_pack(date, ...)` upserts on `date`. A second call
//! for the same date overwrites cleanly — no duplicate rows. This is the
//! contract spelled out in the master plan's Phase 2 exit criteria.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::services::research_notes::{Conviction, EvidenceRef};
use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

#[cfg(test)]
mod tests;

/// One ranked idea inside a pack. `entry_zone` and `invalidation` are
/// short markdown / prose snippets the UI renders verbatim — keeping
/// them as strings (rather than parsed price levels) lets the agent
/// express ranges and conditions without an over-tight schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedIdea {
    pub symbol: String,
    pub thesis_md: String,
    pub conviction: Option<Conviction>,
    pub entry_zone: Option<String>,
    pub invalidation: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

/// Persisted form of an agent-authored morning pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMorningPack {
    pub date: NaiveDate,
    pub ranked_ideas: Vec<RankedIdea>,
    pub written_by: String,
    pub written_at: DateTime<Utc>,
}

#[derive(Error, Debug)]
pub enum AgentMorningPackError {
    #[error("written_by must be non-empty")]
    EmptyWrittenBy,
    #[error("ranked_ideas must be non-empty")]
    EmptyIdeas,
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Inputs for [`write_pack`]. `date` is the trading-day the pack
/// applies to; `ranked_ideas` is rendered in order — the agent decides
/// the ranking.
#[derive(Debug, Clone)]
pub struct NewAgentMorningPack {
    pub date: NaiveDate,
    pub ranked_ideas: Vec<RankedIdea>,
    pub written_by: String,
}

/// Upsert a pack for `date`. Returns the persisted row.
pub async fn write_pack(
    db: &Arc<Db>,
    new: NewAgentMorningPack,
) -> Result<AgentMorningPack, AgentMorningPackError> {
    if new.written_by.trim().is_empty() {
        return Err(AgentMorningPackError::EmptyWrittenBy);
    }
    if new.ranked_ideas.is_empty() {
        return Err(AgentMorningPackError::EmptyIdeas);
    }

    // Normalize each idea's symbol to upper-case.
    let mut ideas = new.ranked_ideas;
    for idea in &mut ideas {
        idea.symbol = idea.symbol.to_uppercase();
    }

    let now_unix = Utc::now().timestamp();
    let now = unix_to_utc(now_unix);
    let date_str = new.date.to_string();
    let payload = serde_json::to_string(&serde_json::json!({
        "ranked_ideas": ideas,
    }))?;
    let written_by_for_db = new.written_by.clone();

    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO agent_morning_packs (date, payload, written_by, written_at) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(date) DO UPDATE SET payload = excluded.payload, \
                                             written_by = excluded.written_by, \
                                             written_at = excluded.written_at",
            rusqlite::params![date_str, payload, written_by_for_db, now_unix],
        )?;
        Ok(())
    })
    .await?;

    Ok(AgentMorningPack {
        date: new.date,
        ranked_ideas: ideas,
        written_by: new.written_by,
        written_at: now,
    })
}

/// Fetch the pack for `date`. Returns `Ok(None)` when no pack exists.
pub async fn get_pack(
    db: &Arc<Db>,
    date: NaiveDate,
) -> Result<Option<AgentMorningPack>, AgentMorningPackError> {
    let date_str = date.to_string();
    let row = db
        .with_conn(move |conn| {
            conn.query_row(
                "SELECT date, payload, written_by, written_at \
                 FROM agent_morning_packs WHERE date = ?1",
                rusqlite::params![date_str],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(StorageError::from)
        })
        .await?;
    match row {
        None => Ok(None),
        Some((d, payload, written_by, ts)) => {
            let date = NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                .map_err(|e| StorageError::Migration(format!("invalid pack date '{d}': {e}")))?;
            let parsed: serde_json::Value = serde_json::from_str(&payload)?;
            let ranked_ideas: Vec<RankedIdea> = match parsed.get("ranked_ideas") {
                Some(v) => serde_json::from_value(v.clone())?,
                None => Vec::new(),
            };
            Ok(Some(AgentMorningPack {
                date,
                ranked_ideas,
                written_by,
                written_at: unix_to_utc(ts),
            }))
        }
    }
}

/// List packs whose `date >= since`, newest-first. Used by the
/// Phase 7 `get_outcomes` tool to walk recent packs and score
/// predictions against realized bars.
pub async fn list_packs_since(
    db: &Arc<Db>,
    since: NaiveDate,
) -> Result<Vec<AgentMorningPack>, AgentMorningPackError> {
    let since_str = since.to_string();
    let rows = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT date, payload, written_by, written_at \
                 FROM agent_morning_packs WHERE date >= ?1 \
                 ORDER BY date DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![since_str], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;

    rows.into_iter()
        .map(|(d, payload, written_by, ts)| {
            let date = NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                .map_err(|e| StorageError::Migration(format!("invalid pack date '{d}': {e}")))?;
            let parsed: serde_json::Value = serde_json::from_str(&payload)?;
            let ranked_ideas: Vec<RankedIdea> = match parsed.get("ranked_ideas") {
                Some(v) => serde_json::from_value(v.clone())?,
                None => Vec::new(),
            };
            Ok(AgentMorningPack {
                date,
                ranked_ideas,
                written_by,
                written_at: unix_to_utc(ts),
            })
        })
        .collect()
}

/// List packs newest-first. Lightweight; the UI shows a compact log of
/// recent agent runs.
pub async fn list_packs(
    db: &Arc<Db>,
    limit: u32,
) -> Result<Vec<AgentMorningPack>, AgentMorningPackError> {
    let limit = limit.max(1) as i64;
    let rows = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT date, payload, written_by, written_at \
                 FROM agent_morning_packs ORDER BY date DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![limit], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;

    rows.into_iter()
        .map(|(d, payload, written_by, ts)| {
            let date = NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                .map_err(|e| StorageError::Migration(format!("invalid pack date '{d}': {e}")))?;
            let parsed: serde_json::Value = serde_json::from_str(&payload)?;
            let ranked_ideas: Vec<RankedIdea> = match parsed.get("ranked_ideas") {
                Some(v) => serde_json::from_value(v.clone())?,
                None => Vec::new(),
            };
            Ok(AgentMorningPack {
                date,
                ranked_ideas,
                written_by,
                written_at: unix_to_utc(ts),
            })
        })
        .collect()
}
