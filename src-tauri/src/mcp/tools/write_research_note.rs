//! `write_research_note` — durable LLM-authored research artifact.
//!
//! Wraps `services::research_notes::write_note`. Validates the closed
//! enum of evidence-ref types (`alert | news | setup | bar_range`) and
//! the A/B/C conviction taxonomy before persisting.

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
use crate::services::research_notes::{self, Conviction, EvidenceRef, NewResearchNote};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteResearchNoteArgs {
    pub symbol: String,
    /// Markdown body of the note. Free-form, rendered verbatim in the UI.
    pub body_md: String,
    /// One of `"A"`, `"B"`, `"C"`. Omit for an unrated note.
    #[serde(default)]
    pub conviction: Option<String>,
    /// Pointers back to the evidence the note rests on. Each entry must
    /// match one of the closed `EvidenceRef` variants:
    /// `{ "type": "alert", "id": N }`,
    /// `{ "type": "news", "cache_id": N }`,
    /// `{ "type": "setup", "id": N }`,
    /// `{ "type": "bar_range", "symbol": "X", "from": "RFC3339", "to": "RFC3339" }`.
    #[serde(default)]
    pub evidence_refs: Option<serde_json::Value>,
    /// Optional `setups.id` to link the note to a specific detector hit.
    #[serde(default)]
    pub setup_id: Option<i64>,
    /// Optional `alerts.id` to link the note to a specific alert.
    #[serde(default)]
    pub alert_id: Option<i64>,
}

#[tool_router(router = write_research_note_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "write_research_note",
        description = "Persist a research note for `symbol`. The body is markdown rendered verbatim in the UI. `conviction` must be one of A/B/C (or omitted). `evidence_refs` is a closed-set list of pointers (alert | news | setup | bar_range). `setup_id` / `alert_id` link the note to a specific detector hit or alert. Returns `{ note_id, symbol, written_at }`."
    )]
    pub async fn write_research_note(
        &self,
        Parameters(args): Parameters<WriteResearchNoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let conviction = match args.conviction.as_deref() {
            None => None,
            Some(s) => match Conviction::parse(s) {
                Some(c) => Some(c),
                None => {
                    return map_tool_result::<(), String>(Err(format!(
                        "conviction must be one of A/B/C, got {s}"
                    )));
                }
            },
        };
        let evidence_refs: Vec<EvidenceRef> = match args.evidence_refs.clone() {
            None => Vec::new(),
            Some(v) => match serde_json::from_value(v) {
                Ok(refs) => refs,
                Err(e) => {
                    return map_tool_result::<(), String>(Err(format!(
                        "evidence_refs invalid: {e}"
                    )));
                }
            },
        };

        let input_for_audit = json!({
            "symbol": args.symbol,
            "conviction": args.conviction,
            "evidence_refs": args.evidence_refs,
            "setup_id": args.setup_id,
            "alert_id": args.alert_id,
            "body_len": args.body_md.len(),
        });
        let audit_id = match record_audit(
            &self.db,
            "write_research_note",
            &input_for_audit,
            &self.caller,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let outcome = research_notes::write_note(
            &self.db,
            NewResearchNote {
                symbol: args.symbol,
                body_md: args.body_md,
                conviction,
                evidence_refs,
                written_by: self.caller.clone(),
                setup_id: args.setup_id,
                alert_id: args.alert_id,
            },
        )
        .await;

        match outcome {
            Ok(saved) => {
                stamp_audit_summary(
                    &self.db,
                    audit_id,
                    &format!("research_notes.id={}", saved.id),
                )
                .await;
                emit_event(
                    &self.emitter,
                    AppEvent::ResearchNoteWritten {
                        note_id: saved.id,
                        symbol: saved.symbol.clone(),
                        alert_id: saved.alert_id,
                        setup_id: saved.setup_id,
                    },
                )
                .await;
                map_tool_result::<_, String>(Ok(json!({
                    "note_id": saved.id,
                    "symbol": saved.symbol,
                    "written_at": saved.written_at,
                })))
            }
            Err(e) => map_tool_result::<(), String>(Err(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    #[tokio::test]
    async fn write_research_note_persists_with_audit_and_event() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                symbol: "TSLA".into(),
                body_md: "## Thesis\nbreakout above 250".into(),
                conviction: Some("A".into()),
                evidence_refs: Some(serde_json::json!([
                    {"type":"alert","id":7},
                    {"type":"news","cache_id":42}
                ])),
                setup_id: None,
                alert_id: None,
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        let note_id = body["note_id"].as_i64().unwrap();
        assert!(note_id > 0);

        // Note persisted.
        let fetched = crate::services::research_notes::get_note(&handler.db, note_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.symbol, "TSLA");
        assert_eq!(fetched.evidence_refs.len(), 2);

        // Audit row points at the note.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].tool, "write_research_note");
        assert_eq!(
            audits[0].result_summary.as_deref(),
            Some(format!("research_notes.id={note_id}").as_str())
        );
    }

    #[tokio::test]
    async fn write_research_note_invalid_conviction_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                symbol: "TSLA".into(),
                body_md: "x".into(),
                conviction: Some("Z".into()),
                evidence_refs: None,
                setup_id: None,
                alert_id: None,
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_research_note_invalid_evidence_ref_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                symbol: "TSLA".into(),
                body_md: "x".into(),
                conviction: None,
                evidence_refs: Some(serde_json::json!([{"type":"twitter","url":"..."}])),
                setup_id: None,
                alert_id: None,
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
