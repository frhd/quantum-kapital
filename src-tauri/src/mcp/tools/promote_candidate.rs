//! `promote_candidate` — Phase 4 write tool.
//!
//! Moves a `candidate_universe` row into the live `tracked_tickers`
//! watchlist. Bypasses the auto-promote score threshold — the caller
//! has explicit reasoning. The candidate doesn't need to exist in
//! staging: the agent can promote a fresh symbol straight from prose
//! and the watchlist row will carry `source = "agent"` either way.
//! The candidate row, when present, gets `promoted_at` stamped so the
//! agent inbox no longer surfaces it.
//!
//! Audit / event semantics mirror `add_ticker` (write_support pattern).

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
use crate::services::candidate_promoter::PromotionOutcome;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PromoteCandidateArgs {
    /// Ticker symbol. Case-insensitive; normalized to upper-case.
    pub symbol: String,
    /// Why the agent is promoting this — short prose, persisted on
    /// the watchlist row's `notes` column.
    pub reason: String,
}

#[tool_router(router = promote_candidate_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "promote_candidate",
        description = "Promote a `candidate_universe` row (or any new symbol) into the tracker watchlist with `source = \"agent\"`. The `reason` is stored as the row's notes; if a candidate row exists, its provenance is copied into the watchlist `source_meta` and `promoted_at` is stamped so the row leaves the agent inbox. Idempotent: re-promoting an already-tracked symbol returns success without churning the watchlist. Returns `{ symbol, status, was_new }`."
    )]
    pub async fn promote_candidate(
        &self,
        Parameters(args): Parameters<PromoteCandidateArgs>,
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
        let audit_id = match record_audit(&self.db, "promote_candidate", &input, &self.caller).await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let outcome = match self
            .candidate_promoter
            .promote_for_agent(&symbol, &args.reason)
            .await
        {
            Ok(o) => o,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let (was_new, summary) = match outcome {
            PromotionOutcome::Promoted => (true, format!("tracked_tickers.symbol={symbol}")),
            PromotionOutcome::AlreadyTracked => (false, "already_tracked".to_string()),
            // Agent path doesn't gate on threshold/cooldown; surface
            // these as errors so the caller knows nothing happened.
            PromotionOutcome::BelowThreshold { score, threshold } => {
                return map_tool_result::<(), String>(Err(format!(
                    "promotion gated unexpectedly: score {score:.2} < threshold {threshold:.2}"
                )));
            }
            PromotionOutcome::InCooldown { until } => {
                return map_tool_result::<(), String>(Err(format!(
                    "promotion gated unexpectedly: in cooldown until {until}"
                )));
            }
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

        map_tool_result::<_, String>(Ok(json!({
            "symbol": symbol,
            "status": "watching",
            "was_new": was_new,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use crate::services::candidate_universe::types::{CandidateSource, NewCandidate};

    async fn seed_candidate(handler: &McpHandler, symbol: &str, score: f64) {
        handler
            .candidates
            .upsert(NewCandidate {
                symbol: symbol.to_string(),
                source: CandidateSource {
                    source: "scanner_top_perc_gain".into(),
                    score,
                    rank: Some(3),
                    meta: serde_json::json!({}),
                    last_seen: 0,
                },
                reason_md: Some(format!("seed {symbol}")),
                ttl_seconds: 7 * 86_400,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn promote_candidate_persists_with_agent_source_and_stamps_candidate() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        seed_candidate(&handler, "TSLA", 0.4).await;

        let r = handler
            .promote_candidate(Parameters(PromoteCandidateArgs {
                symbol: "tsla".into(),
                reason: "earnings tomorrow".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["symbol"], "TSLA");
        assert_eq!(body["was_new"], true);

        // Watchlist row carries source=agent + reason in notes.
        let row = handler.tracker.get("TSLA").await.unwrap().unwrap();
        assert_eq!(row.source.as_str(), "agent");
        assert_eq!(row.notes.as_deref(), Some("earnings tomorrow"));

        // Candidate row got `promoted_at` stamped.
        let after = handler.candidates.get("TSLA").await.unwrap().unwrap();
        assert!(after.promoted_at.is_some());

        // Audit row landed.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].tool, "promote_candidate");
        assert!(audits[0].result_summary.is_some());
    }

    #[tokio::test]
    async fn promote_candidate_works_for_unknown_symbol() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        // No candidate seeded — agent is going off-script.
        let r = handler
            .promote_candidate(Parameters(PromoteCandidateArgs {
                symbol: "NEW".into(),
                reason: "intuition".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.unwrap();
        assert_eq!(body["was_new"], true);
        assert_eq!(
            handler
                .tracker
                .get("NEW")
                .await
                .unwrap()
                .unwrap()
                .source
                .as_str(),
            "agent"
        );
        assert!(handler.candidates.get("NEW").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn promote_candidate_idempotent_returns_was_new_false() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        seed_candidate(&handler, "AAPL", 0.4).await;
        let _ = handler
            .promote_candidate(Parameters(PromoteCandidateArgs {
                symbol: "AAPL".into(),
                reason: "x".into(),
            }))
            .await
            .expect("ok");
        let r2 = handler
            .promote_candidate(Parameters(PromoteCandidateArgs {
                symbol: "AAPL".into(),
                reason: "x".into(),
            }))
            .await
            .expect("ok");
        let body = r2.structured_content.unwrap();
        assert_eq!(body["was_new"], false);

        // Two audit rows — both writes recorded even when the second was a no-op.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 2);
    }

    #[tokio::test]
    async fn promote_candidate_blank_inputs_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .promote_candidate(Parameters(PromoteCandidateArgs {
                symbol: "  ".into(),
                reason: "x".into(),
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));

        let r = handler
            .promote_candidate(Parameters(PromoteCandidateArgs {
                symbol: "TSLA".into(),
                reason: "  ".into(),
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
