//! `ack_alert` — record a decision against an alert.
//!
//! Wraps `services::alerts::ack_alert`. Decision is one of
//! `acted | passed | researching`. Optional `note` becomes a
//! `research_notes` row linked to the alert.

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
use crate::services::alerts::{ack_alert, AlertDecision};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AckAlertArgs {
    pub alert_id: i64,
    /// One of `"acted"`, `"passed"`, `"researching"`.
    pub decision: String,
    /// Optional free-text note. When non-blank, persists a
    /// `research_notes` row linked to the alert and stamps its id on
    /// `alerts.decision_note_id`.
    #[serde(default)]
    pub note: Option<String>,
}

#[tool_router(router = ack_alert_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "ack_alert",
        description = "Close out an alert with a decision (`acted` | `passed` | `researching`). When `note` is non-blank, the rail also creates a research_note linked to the alert so the eval harness can later compare decisions against subsequent setup outcomes. Returns `{ alert_id, decision, note_id }`."
    )]
    pub async fn ack_alert(
        &self,
        Parameters(args): Parameters<AckAlertArgs>,
    ) -> Result<CallToolResult, McpError> {
        let decision = match AlertDecision::parse(&args.decision) {
            Some(d) => d,
            None => {
                return map_tool_result::<(), String>(Err(format!(
                    "decision must be one of acted/passed/researching, got {}",
                    args.decision
                )));
            }
        };

        let input_for_audit = json!({
            "alert_id": args.alert_id,
            "decision": args.decision,
            "has_note": args.note.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false),
        });
        let audit_id = match record_audit(
            &self.db,
            "ack_alert",
            &input_for_audit,
            &self.caller,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let outcome = ack_alert(
            &self.db,
            args.alert_id,
            decision,
            args.note.as_deref(),
            &self.caller,
        )
        .await;

        match outcome {
            Ok(o) => {
                let note_id = o.note.as_ref().map(|n| n.id);
                stamp_audit_summary(
                    &self.db,
                    audit_id,
                    &format!(
                        "alert_id={}, note_id={}",
                        o.alert_id,
                        note_id
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "none".to_string())
                    ),
                )
                .await;
                emit_event(
                    &self.emitter,
                    AppEvent::AlertDecisionRecorded {
                        alert_id: o.alert_id,
                        decision: decision.as_str().to_string(),
                        note_id,
                    },
                )
                .await;
                map_tool_result::<_, String>(Ok(json!({
                    "alert_id": o.alert_id,
                    "decision": decision.as_str(),
                    "note_id": note_id,
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
    use crate::strategies::{Direction, SetupCandidate, TargetLevel};

    async fn seed_alert(handler: &McpHandler, symbol: &str) -> i64 {
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
        let setup = handler.tracker.insert_setup(symbol, &candidate).await.unwrap();
        record_alert(&handler.db, setup.id, AlertKind::Detected, json!({}))
            .await
            .unwrap()
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn ack_alert_with_note_creates_linked_note() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let alert_id = seed_alert(&handler, "TSLA").await;

        let r = handler
            .ack_alert(Parameters(AckAlertArgs {
                alert_id,
                decision: "acted".into(),
                note: Some("opened starter long".into()),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["decision"].as_str().unwrap(), "acted");
        let note_id = body["note_id"].as_i64().expect("note_id present");
        assert!(note_id > 0);

        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits[0].tool, "ack_alert");
    }

    #[tokio::test]
    async fn ack_alert_invalid_decision_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let alert_id = seed_alert(&handler, "TSLA").await;
        let r = handler
            .ack_alert(Parameters(AckAlertArgs {
                alert_id,
                decision: "yolo".into(),
                note: None,
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
