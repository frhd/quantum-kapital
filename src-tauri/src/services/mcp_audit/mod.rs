//! Phase 02 — append-only audit log for every MCP write.
//!
//! Enforces the master-plan hard invariant *"every MCP write is audited"*.
//! Every write tool in `mcp/tools/writes.rs` calls [`record`] **before** it
//! mutates state, so an aborted mutation still leaves a row visible to the
//! UI's eval/audit dashboards.
//!
//! This module is intentionally a pair of free functions over `&Arc<Db>`
//! rather than a stateful service struct. There is no in-memory state to
//! own and no trait seam needed: tests construct the same `Db` the live
//! handler uses.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::error::StorageError;
use crate::storage::Db;
use crate::utils::helpers::unix_to_utc;

#[cfg(test)]
mod tests;

/// One persisted row in `mcp_audit`. Returned by [`list`] for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpAuditEntry {
    pub id: i64,
    pub tool: String,
    pub input: serde_json::Value,
    pub result_summary: Option<String>,
    pub caller: Option<String>,
    pub called_at: DateTime<Utc>,
}

/// Append a row to `mcp_audit`. Returns the new row's `id` so the caller
/// can stitch the audit row to the artifact it produced (e.g. a
/// `research_notes.id`) in `result_summary` if useful.
///
/// Errors are surfaced verbatim — the surrounding write tool has to decide
/// whether to abort the mutation or proceed with a logged-but-unobserved
/// audit.
pub async fn record(
    db: &Arc<Db>,
    tool: &str,
    input: &serde_json::Value,
    result_summary: Option<&str>,
    caller: Option<&str>,
) -> Result<i64, StorageError> {
    let tool = tool.to_string();
    let input_str = serde_json::to_string(input).map_err(StorageError::from)?;
    let summary = result_summary.map(|s| s.to_string());
    let caller = caller.map(|s| s.to_string());
    let called_at_unix = Utc::now().timestamp();

    let id = db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT INTO mcp_audit (tool, input, result_summary, caller, called_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![tool, input_str, summary, caller, called_at_unix],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .await?;

    Ok(id)
}

/// Read the most recent audit rows, newest first. `limit` is clamped to at
/// least 1 to keep the SQL safe; `offset` is forwarded as-is.
pub async fn list(
    db: &Arc<Db>,
    limit: u32,
    offset: u32,
) -> Result<Vec<McpAuditEntry>, StorageError> {
    let limit = limit.max(1) as i64;
    let offset = offset as i64;
    let rows = db
        .with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, tool, input, result_summary, caller, called_at \
                 FROM mcp_audit ORDER BY called_at DESC, id DESC LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![limit, offset], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;

    rows.into_iter()
        .map(|(id, tool, input_s, summary, caller, called_at)| {
            let input: serde_json::Value =
                serde_json::from_str(&input_s).map_err(StorageError::from)?;
            Ok(McpAuditEntry {
                id,
                tool,
                input,
                result_summary: summary,
                caller,
                called_at: unix_to_utc(called_at),
            })
        })
        .collect()
}
