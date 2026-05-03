//! `add_ticker` — agent / interactive write tool.
//!
//! Adds a symbol to the tracker watchlist with `source = "agent"` and
//! the supplied `reason` stashed in the watchlist row's `notes`.
//! Idempotent: re-adding an already-tracked symbol returns success
//! with the existing row, never an error — the agent is allowed to
//! call this defensively.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::events::AppEvent;
use crate::ibkr::types::tracker::{TrackerSource, TrackerStatus};
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::mcp::tools::write_support::{emit_event, record_audit, stamp_audit_summary};
use crate::services::tracker_service::TrackerError;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddTickerArgs {
    /// Ticker symbol. Case-insensitive; normalized to upper-case.
    pub symbol: String,
    /// Why this symbol is being added — short prose, persisted on the
    /// watchlist row's `notes` column.
    pub reason: String,
}

#[tool_router(router = add_ticker_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "add_ticker",
        description = "Add a symbol to the tracker watchlist with source=\"agent\". The `reason` is stored as the row's notes so a human reviewing the watchlist later sees why the agent promoted it. Idempotent: calling on an already-tracked symbol returns success without changing anything. Returns `{ symbol, source, status, was_new }`."
    )]
    pub async fn add_ticker(
        &self,
        Parameters(args): Parameters<AddTickerArgs>,
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
        let audit_id = match record_audit(&self.db, "add_ticker", &input, &self.caller).await {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let outcome = self
            .tracker
            .add(
                &symbol,
                TrackerSource::Agent,
                None,
                vec![],
                Some(args.reason.clone()),
            )
            .await;

        let (was_new, summary) = match outcome {
            Ok(_) => (true, format!("tracked_tickers.symbol={symbol}")),
            Err(TrackerError::AlreadyTracked(_)) => (false, "already_tracked".to_string()),
            Err(e) => return map_tool_result::<(), String>(Err(e.to_string())),
        };
        stamp_audit_summary(&self.db, audit_id, &summary).await;
        emit_event(
            &self.emitter,
            AppEvent::TickerStatusChanged {
                symbol: symbol.clone(),
                from: TrackerStatus::Watching,
                to: TrackerStatus::Watching,
            },
        )
        .await;

        // Ticker-intake Phase 1: spawn the post-add prime chain so the
        // projection / news panels can warm without blocking the MCP
        // response. The primer is idempotent on `last_primed_at < 24h`
        // so a re-add (`was_new = false`) short-circuits inside `prime`
        // — we still spawn rather than branch here so the `was_new`
        // semantics belong to the caller, not the primer wiring.
        let primer = std::sync::Arc::clone(&self.primer);
        let symbol_for_primer = symbol.clone();
        tokio::spawn(async move {
            primer.prime(&symbol_for_primer).await;
        });

        map_tool_result::<_, String>(Ok(json!({
            "symbol": symbol,
            "source": "agent",
            "status": "watching",
            "was_new": was_new,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    #[tokio::test]
    async fn add_ticker_persists_with_agent_source_and_audits() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let result = handler
            .add_ticker(Parameters(AddTickerArgs {
                symbol: "tsla".into(),
                reason: "scanner_top_gainer".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(result.is_error, Some(false));
        let body = result.structured_content.expect("structured");
        assert_eq!(body["symbol"].as_str().unwrap(), "TSLA");
        assert_eq!(body["source"].as_str().unwrap(), "agent");
        assert!(body["was_new"].as_bool().unwrap());

        // Tracker row exists with the agent source + reason as notes.
        let row = handler.tracker.get("TSLA").await.unwrap().unwrap();
        assert_eq!(row.source.as_str(), "agent");
        assert_eq!(row.notes.as_deref(), Some("scanner_top_gainer"));

        // Audit row landed.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].tool, "add_ticker");
        assert!(audits[0].result_summary.is_some());
    }

    #[tokio::test]
    async fn add_ticker_is_idempotent() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r1 = handler
            .add_ticker(Parameters(AddTickerArgs {
                symbol: "AAPL".into(),
                reason: "x".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(r1.structured_content.unwrap()["was_new"], true);

        let r2 = handler
            .add_ticker(Parameters(AddTickerArgs {
                symbol: "AAPL".into(),
                reason: "x".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(r2.is_error, Some(false));
        assert_eq!(r2.structured_content.unwrap()["was_new"], false);

        // Only one persisted row.
        let listed = handler.tracker.list(None).await.unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn add_ticker_blank_inputs_error_without_audit_when_validation_fails_first() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .add_ticker(Parameters(AddTickerArgs {
                symbol: "  ".into(),
                reason: "x".into(),
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
        // Validation rejection short-circuits before the audit so the table
        // is empty.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert!(audits.is_empty());
    }

    /// Phase 1 ticker-intake exit criterion: after `add_ticker` returns,
    /// the spawned primer task must complete and emit
    /// `AppEvent::TickerPrimingDone` for the same symbol, and the
    /// watchlist row must carry `last_primed_at`. The test seam wires a
    /// `FakeFundamentalsProvider` (returns `NotFound` ⇒ NoData step) and
    /// a `FakeNewsProvider` (returns `Ok(empty)` ⇒ NoData step), so the
    /// outcome is `(NoData, NoData, NoData)` — but `last_primed_at` is
    /// still stamped because the fundamentals call itself completed
    /// (the primer's "what counts as primed?" rule).
    #[tokio::test]
    async fn add_ticker_spawns_primer_and_emits_priming_done() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let _ = handler
            .add_ticker(Parameters(AddTickerArgs {
                symbol: "AAPL".into(),
                reason: "ticker-intake spawn check".into(),
            }))
            .await
            .expect("ok");

        // Spawned primer runs on the same runtime; poll until the
        // capture buffer carries the event or the deadline trips.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let primed = loop {
            let events = handler.emitter.captured().await;
            if events.iter().any(
                |e| matches!(e, AppEvent::TickerPrimingDone { symbol, .. } if symbol == "AAPL"),
            ) {
                break true;
            }
            if std::time::Instant::now() > deadline {
                break false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        };
        assert!(
            primed,
            "TickerPrimingDone must be emitted after add_ticker returns"
        );

        let row = handler.tracker.get("AAPL").await.unwrap().expect("present");
        assert!(
            row.last_primed_at.is_some(),
            "post-add prime must stamp last_primed_at on the watchlist row"
        );
    }
}
