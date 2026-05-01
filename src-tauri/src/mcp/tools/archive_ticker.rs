//! `archive_ticker` — soft-archive a tracked symbol via MCP.
//!
//! Wraps the existing `TrackerService::archive_ticker` rail (commit
//! `74969a8`). The `reason` is appended to the tracker row's `notes`
//! before the archive so the audit trail explains *why* a symbol was
//! pulled. Idempotent: archiving an already-archived symbol returns
//! success without changing the timestamp.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::events::AppEvent;
use crate::ibkr::types::tracker::TrackerStatus;
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::mcp::tools::write_support::{emit_event, record_audit, stamp_audit_summary};
use crate::services::tracker_service::TrackerError;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArchiveTickerArgs {
    /// Symbol to archive. Case-insensitive.
    pub symbol: String,
    /// Why this symbol is being archived — short prose for the audit log.
    pub reason: String,
}

#[tool_router(router = archive_ticker_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "archive_ticker",
        description = "Soft-archive a tracked symbol so it drops out of the watchlist, detector runs, the state machine, and alert emission. The `reason` is recorded in the audit log. Idempotent and reversible (a future `unarchive_ticker` rail can restore it). Returns `{ symbol, archived: true }`."
    )]
    pub async fn archive_ticker(
        &self,
        Parameters(args): Parameters<ArchiveTickerArgs>,
    ) -> Result<CallToolResult, McpError> {
        let symbol_trim = args.symbol.trim();
        if symbol_trim.is_empty() {
            return map_tool_result::<(), String>(Err("symbol must be non-empty".to_string()));
        }
        if args.reason.trim().is_empty() {
            return map_tool_result::<(), String>(Err("reason must be non-empty".to_string()));
        }
        let symbol = symbol_trim.to_uppercase();

        let input = json!({"symbol": symbol, "reason": args.reason});
        let audit_id = match record_audit(&self.db, "archive_ticker", &input, &self.caller).await {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let outcome = self.tracker.archive_ticker(&symbol).await;
        match outcome {
            Ok(()) => {
                stamp_audit_summary(&self.db, audit_id, &format!("archived symbol={symbol}")).await;
                emit_event(
                    &self.emitter,
                    AppEvent::TickerStatusChanged {
                        symbol: symbol.clone(),
                        from: TrackerStatus::Watching,
                        to: TrackerStatus::CoolDown,
                    },
                )
                .await;
                map_tool_result::<_, String>(Ok(json!({
                    "symbol": symbol,
                    "archived": true,
                })))
            }
            Err(TrackerError::NotFound(s)) => {
                map_tool_result::<(), String>(Err(format!("symbol {s} is not tracked")))
            }
            Err(e) => map_tool_result::<(), String>(Err(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::tracker::TrackerSource;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    #[tokio::test]
    async fn archive_ticker_archives_and_audits() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        handler
            .tracker
            .add("TSLA", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();

        let r = handler
            .archive_ticker(Parameters(ArchiveTickerArgs {
                symbol: "tsla".into(),
                reason: "thesis_invalidated".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));

        // Tracker row no longer surfaces in `list`.
        let listed = handler.tracker.list(None).await.unwrap();
        assert!(listed.is_empty());

        // Audit row exists.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].tool, "archive_ticker");
    }

    #[tokio::test]
    async fn archive_ticker_unknown_symbol_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .archive_ticker(Parameters(ArchiveTickerArgs {
                symbol: "NOSUCH".into(),
                reason: "x".into(),
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
