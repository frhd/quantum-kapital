//! Phase 02 — shared scaffolding for MCP write tools.
//!
//! Two responsibilities:
//!
//! 1. **Audit-before-mutate.** Every write tool calls
//!    [`record_audit`] *before* it touches state, satisfying the
//!    master-plan invariant "every MCP write is audited" — even an
//!    aborted mutation leaves a row visible to the eval/audit dashboards.
//!
//! 2. **Best-effort event emission.** [`emit_event`] forwards an
//!    `AppEvent` to the shared `EventEmitter` and logs (rather than
//!    propagates) any error: a failed UI broadcast must never roll back
//!    a successful mutation.

use std::sync::Arc;

use serde_json::Value;
use tracing::warn;

use crate::events::{AppEvent, EventEmitter};
use crate::services::mcp_audit;
use crate::storage::Db;

/// Append a row to `mcp_audit` for the upcoming write. Must be called
/// **before** the underlying mutation. A failed audit short-circuits
/// the write tool with the storage error so the surveillance audit
/// invariant is never silently broken.
pub async fn record_audit(
    db: &Arc<Db>,
    tool: &str,
    input: &Value,
    caller: &str,
) -> Result<i64, String> {
    mcp_audit::record(db, tool, input, None, Some(caller))
        .await
        .map_err(|e| format!("audit insert failed: {e}"))
}

/// Stamp the audit row with a short result summary (e.g. the new
/// artifact's id) once the mutation succeeds. Errors here are logged,
/// not propagated — the write itself already landed and the audit row
/// already exists; only the summary annotation failed.
pub async fn stamp_audit_summary(db: &Arc<Db>, audit_id: i64, summary: &str) {
    let summary = summary.to_string();
    let res = db
        .with_conn(move |conn| {
            conn.execute(
                "UPDATE mcp_audit SET result_summary = ?1 WHERE id = ?2",
                rusqlite::params![summary, audit_id],
            )?;
            Ok(())
        })
        .await;
    if let Err(e) = res {
        warn!("mcp_audit summary stamp failed (audit_id={audit_id}): {e}");
    }
}

/// Forward an `AppEvent`. Best-effort — a failed broadcast must not
/// roll back the mutation.
pub async fn emit_event(emitter: &Arc<EventEmitter>, event: AppEvent) {
    if let Err(e) = emitter.emit(event).await {
        warn!("MCP write tool event emit failed: {e}");
    }
}
