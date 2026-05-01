//! Phase 02 — Tauri commands for the research-artifact UI surface.
//!
//! Read-only views over the rows the MCP write tools persist. The
//! frontend's research view calls these to render notes / morning packs
//! and to inspect the audit log.

use std::sync::Arc;

use chrono::NaiveDate;
use tauri::State;

use crate::services::agent_morning_packs::{self, AgentMorningPack};
use crate::services::mcp_audit::{self, McpAuditEntry};
use crate::services::research_notes::{self, ListNotesQuery, ResearchNote};
use crate::storage::Db;

/// List research notes, newest-first, with optional filters.
#[tauri::command]
pub async fn research_list_notes(
    db: State<'_, Arc<Db>>,
    symbol: Option<String>,
    setup_id: Option<i64>,
    alert_id: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ResearchNote>, String> {
    let query = ListNotesQuery {
        symbol,
        setup_id,
        alert_id,
        limit: limit.unwrap_or(50),
        offset: offset.unwrap_or(0),
    };
    research_notes::list_notes(&db, query)
        .await
        .map_err(|e| e.to_string())
}

/// Fetch a single note by id.
#[tauri::command]
pub async fn research_get_note(
    db: State<'_, Arc<Db>>,
    id: i64,
) -> Result<Option<ResearchNote>, String> {
    research_notes::get_note(&db, id)
        .await
        .map_err(|e| e.to_string())
}

/// Fetch the agent-authored morning pack for a date.
#[tauri::command]
pub async fn research_get_agent_morning_pack(
    db: State<'_, Arc<Db>>,
    date: String,
) -> Result<Option<AgentMorningPack>, String> {
    let parsed = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|e| format!("date must be YYYY-MM-DD: {e}"))?;
    agent_morning_packs::get_pack(&db, parsed)
        .await
        .map_err(|e| e.to_string())
}

/// List recent agent morning packs newest-first.
#[tauri::command]
pub async fn research_list_agent_morning_packs(
    db: State<'_, Arc<Db>>,
    limit: Option<u32>,
) -> Result<Vec<AgentMorningPack>, String> {
    agent_morning_packs::list_packs(&db, limit.unwrap_or(20))
        .await
        .map_err(|e| e.to_string())
}

/// Read recent MCP audit rows. Newest-first.
#[tauri::command]
pub async fn research_list_mcp_audit(
    db: State<'_, Arc<Db>>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<McpAuditEntry>, String> {
    mcp_audit::list(&db, limit.unwrap_or(50), offset.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}
