//! Phase 6 — `mark_alert_enriched` write tool.
//!
//! Idempotent marker stamped by the alert-dive agent (or a manual ack
//! flow) once a research_note has been written for an alert. Re-calling
//! for the same `alert_id` is a no-op that returns the original stamp.
//!
//! Audit-before-mutate per the master plan; emits `AlertEnriched` so
//! the React alert-detail panel flips state without reloading the feed.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::events::AppEvent;
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::mcp::tools::write_support::{emit_event, record_audit, stamp_audit_summary};
use crate::services::alerts::mark_alert_enriched;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarkAlertEnrichedArgs {
    pub alert_id: i64,
    /// `research_notes.id` linked to this enrichment. Omit to record a
    /// "skipped" enrichment (e.g. global budget exhausted) without
    /// manufacturing a note.
    #[serde(default)]
    pub research_note_id: Option<i64>,
}

#[tool_router(router = mark_alert_enriched_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "mark_alert_enriched",
        description = "Stamp `alerts.enriched_at = now` and (optionally) `alerts.research_note_id`. Idempotent — a second call for the same `alert_id` returns the existing stamp without overwriting. Pass the id of a previously written research_note via `write_research_note`, or omit it to record a skipped enrichment. Returns `{ alert_id, enriched_at, research_note_id, newly_marked }`."
    )]
    pub async fn mark_alert_enriched(
        &self,
        Parameters(args): Parameters<MarkAlertEnrichedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let input_for_audit = json!({
            "alert_id": args.alert_id,
            "research_note_id": args.research_note_id,
        });
        let audit_id = match record_audit(
            &self.db,
            "mark_alert_enriched",
            &input_for_audit,
            &self.caller,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let outcome = mark_alert_enriched(&self.db, args.alert_id, args.research_note_id).await;

        match outcome {
            Ok(o) => {
                stamp_audit_summary(
                    &self.db,
                    audit_id,
                    &format!(
                        "alert_id={}, note_id={}, newly_marked={}",
                        o.alert_id,
                        o.research_note_id
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        o.newly_marked
                    ),
                )
                .await;
                if o.newly_marked {
                    emit_event(
                        &self.emitter,
                        AppEvent::AlertEnriched {
                            alert_id: o.alert_id,
                            research_note_id: o.research_note_id,
                        },
                    )
                    .await;
                }
                map_tool_result::<_, String>(Ok(json!({
                    "alert_id": o.alert_id,
                    "enriched_at": o.enriched_at.to_rfc3339(),
                    "research_note_id": o.research_note_id,
                    "newly_marked": o.newly_marked,
                })))
            }
            Err(e) => map_tool_result::<(), String>(Err(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use serde_json::json;

    use crate::ibkr::types::tracker::{AlertKind, StrategyTag, TrackerSource};
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use crate::services::alerts::record_alert;
    use crate::services::research_notes::{self, NewResearchNote};
    use crate::strategies::{Direction, SetupCandidate, TargetLevel};

    async fn seed(handler: &McpHandler, symbol: &str) -> (i64, i64) {
        handler
            .tracker
            .add(symbol, TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        let candidate = SetupCandidate {
            strategy: "breakout",
            tag: StrategyTag::Breakout,
            direction: Direction::Long,
            conviction_signal: 0.7,
            trigger_price: 100.0,
            stop_price: 95.0,
            targets: vec![TargetLevel {
                label: "T1".to_string(),
                price: 110.0,
            }],
            raw_signals: json!({}),
            timeframe: crate::ibkr::types::historical::BarSize::Day1,
            detected_at: Utc::now(),
        };
        let setup = handler
            .tracker
            .insert_setup(symbol, &candidate)
            .await
            .unwrap();
        let alert = record_alert(&handler.db, setup.id, AlertKind::Detected, json!({}))
            .await
            .unwrap()
            .unwrap();
        let note = research_notes::write_note(
            &handler.db,
            NewResearchNote {
                symbol: symbol.to_string(),
                body_md: "deep dive".to_string(),
                conviction: None,
                evidence_refs: vec![],
                written_by: "agent_alert_dive".to_string(),
                setup_id: Some(setup.id),
                alert_id: Some(alert.id),
                price_at_write: None,
                invalidation_price: None,
                invalidation_kind: None,
                targets: vec![],
                catalyst_date: None,
            },
        )
        .await
        .unwrap();
        (alert.id, note.id)
    }

    #[tokio::test]
    async fn first_call_marks_and_audits() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let (alert_id, note_id) = seed(&handler, "AAPL").await;

        let r = handler
            .mark_alert_enriched(Parameters(MarkAlertEnrichedArgs {
                alert_id,
                research_note_id: Some(note_id),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["alert_id"].as_i64().unwrap(), alert_id);
        assert_eq!(body["research_note_id"].as_i64().unwrap(), note_id);
        assert!(body["newly_marked"].as_bool().unwrap());

        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert!(audits.iter().any(|a| a.tool == "mark_alert_enriched"));
    }

    #[tokio::test]
    async fn second_call_is_idempotent() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let (alert_id, note_id) = seed(&handler, "MSFT").await;

        handler
            .mark_alert_enriched(Parameters(MarkAlertEnrichedArgs {
                alert_id,
                research_note_id: Some(note_id),
            }))
            .await
            .unwrap();
        let r = handler
            .mark_alert_enriched(Parameters(MarkAlertEnrichedArgs {
                alert_id,
                research_note_id: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert!(!body["newly_marked"].as_bool().unwrap());
        // First call's note id stays.
        assert_eq!(body["research_note_id"].as_i64().unwrap(), note_id);
    }

    #[tokio::test]
    async fn unknown_alert_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .mark_alert_enriched(Parameters(MarkAlertEnrichedArgs {
                alert_id: 99_999,
                research_note_id: None,
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
