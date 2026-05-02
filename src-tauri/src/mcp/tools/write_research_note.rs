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
use crate::services::research_notes::{
    self, Conviction, EvidenceRef, InvalidationKind, NewResearchNote, NoteTarget,
};

/// One author-asserted price target. Mirrors `NoteTarget` but lives here
/// so `schemars` can derive a JSON schema without adding the dependency
/// to the service module.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteResearchNoteTarget {
    /// Display label, typically `"T1"` / `"T2"`. Free-form so an author
    /// can write `"gap fill"` if they prefer.
    pub label: String,
    /// Absolute price the target is hit at.
    pub price: f64,
}

impl From<WriteResearchNoteTarget> for NoteTarget {
    fn from(t: WriteResearchNoteTarget) -> Self {
        NoteTarget {
            label: t.label,
            price: t.price,
        }
    }
}

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
    /// Last price observed when the note was written. If omitted, the
    /// tool snapshots `QuoteService::fetch_quote` at write time and
    /// stores its `last_price` here. Powers the "drift since write"
    /// readout in the UI.
    #[serde(default)]
    pub price_at_write: Option<f64>,
    /// Level the thesis dies at (e.g. "close back below $156"). Pair
    /// with `invalidation_kind`. The UI compares this against the live
    /// quote to decide breach.
    #[serde(default)]
    pub invalidation_price: Option<f64>,
    /// Direction of invalidation: `"close_below"` (long bias),
    /// `"close_above"` (short bias), or `"intraday_breach"` (any
    /// intraday print past the level). Required when
    /// `invalidation_price` is set; ignored otherwise.
    #[serde(default)]
    pub invalidation_kind: Option<String>,
    /// Ordered author-asserted price targets. Order is the rank: T1
    /// first, T2 second. Capped at 4 entries.
    #[serde(default)]
    pub targets: Option<Vec<WriteResearchNoteTarget>>,
    /// ISO date for a known upcoming catalyst (earnings, FDA, etc.).
    /// Optional.
    #[serde(default)]
    pub catalyst_date: Option<String>,
}

#[tool_router(router = write_research_note_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "write_research_note",
        description = "Persist a research note for `symbol`. The body is markdown rendered in the UI (GFM tables, headings, bold, etc.). `conviction` must be one of A/B/C (or omitted). `evidence_refs` is a closed-set list of pointers (alert | news | setup | bar_range). `setup_id` / `alert_id` link the note to a specific detector hit or alert. \
                       \n\nIf the body discusses invalidation or targets, also populate the structured fields — they power the Research-tab \"thesis at a glance\" card that compares the live quote against your levels. \
                       `price_at_write` (number, USD) anchors a price-drift readout; if omitted, the tool snapshots the live quote at write time. \
                       `invalidation_price` (number) + `invalidation_kind` (`close_below` | `close_above` | `intraday_breach`) define the level the thesis dies at. \
                       `targets` (array of `{label, price}`, max 4) lists ranked targets — T1 first. \
                       `catalyst_date` (`YYYY-MM-DD`) marks a known upcoming event. \
                       Returns `{ note_id, symbol, written_at }`."
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

        let invalidation_kind = match args.invalidation_kind.as_deref() {
            None => None,
            Some(s) => match InvalidationKind::parse(s) {
                Some(k) => Some(k),
                None => {
                    return map_tool_result::<(), String>(Err(format!(
                        "invalidation_kind must be one of close_below/close_above/intraday_breach, got {s}"
                    )));
                }
            },
        };
        if args.invalidation_price.is_some() && invalidation_kind.is_none() {
            return map_tool_result::<(), String>(Err(
                "invalidation_kind is required when invalidation_price is set".to_string(),
            ));
        }

        let targets: Vec<NoteTarget> = args
            .targets
            .unwrap_or_default()
            .into_iter()
            .map(NoteTarget::from)
            .collect();
        if targets.len() > 4 {
            return map_tool_result::<(), String>(Err(format!(
                "targets capped at 4 entries, got {}",
                targets.len()
            )));
        }

        // Auto-snapshot the live price when the author didn't provide
        // one. Best-effort — a TWS disconnect or quote permission error
        // must not block the write. Symbol normalization mirrors the
        // service's uppercasing so the `quote_service` lookup hits the
        // same key the `research_notes` row will be filed under.
        let price_at_write = match args.price_at_write {
            Some(p) => Some(p),
            None => {
                let symbol_upper = args.symbol.trim().to_uppercase();
                if symbol_upper.is_empty() {
                    None
                } else {
                    match self.quote_service.fetch_quote(&symbol_upper).await {
                        Ok(q) => q.last_price,
                        Err(e) => {
                            tracing::debug!(
                                symbol = %symbol_upper,
                                error = %e,
                                "write_research_note: live-quote snapshot failed; storing note without price_at_write"
                            );
                            None
                        }
                    }
                }
            }
        };

        let input_for_audit = json!({
            "symbol": args.symbol,
            "conviction": args.conviction,
            "evidence_refs": args.evidence_refs,
            "setup_id": args.setup_id,
            "alert_id": args.alert_id,
            "body_len": args.body_md.len(),
            "price_at_write": price_at_write,
            "invalidation_price": args.invalidation_price,
            "invalidation_kind": args.invalidation_kind,
            "target_count": targets.len(),
            "catalyst_date": args.catalyst_date,
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
                price_at_write,
                invalidation_price: args.invalidation_price,
                invalidation_kind,
                targets,
                catalyst_date: args.catalyst_date,
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
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::mcp::tools::test_support::{handler_for_db, handler_for_mock_ibkr, make_db};
    use std::sync::Arc;

    fn args(symbol: &str, body: &str) -> WriteResearchNoteArgs {
        WriteResearchNoteArgs {
            symbol: symbol.into(),
            body_md: body.into(),
            conviction: None,
            evidence_refs: None,
            setup_id: None,
            alert_id: None,
            price_at_write: None,
            invalidation_price: None,
            invalidation_kind: None,
            targets: None,
            catalyst_date: None,
        }
    }

    #[tokio::test]
    async fn write_research_note_persists_with_audit_and_event() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                conviction: Some("A".into()),
                evidence_refs: Some(serde_json::json!([
                    {"type":"alert","id":7},
                    {"type":"news","cache_id":42}
                ])),
                ..args("TSLA", "## Thesis\nbreakout above 250")
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
                conviction: Some("Z".into()),
                ..args("TSLA", "x")
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
                evidence_refs: Some(serde_json::json!([{"type":"twitter","url":"..."}])),
                ..args("TSLA", "x")
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_research_note_persists_structured_levels() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                price_at_write: Some(166.48),
                invalidation_price: Some(156.0),
                invalidation_kind: Some("close_below".into()),
                targets: Some(vec![
                    WriteResearchNoteTarget {
                        label: "T1".into(),
                        price: 185.0,
                    },
                    WriteResearchNoteTarget {
                        label: "T2".into(),
                        price: 215.0,
                    },
                ]),
                catalyst_date: Some("2026-05-15".into()),
                ..args("RDDT", "## Thesis\ngap up continuation")
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let note_id = r.structured_content.unwrap()["note_id"].as_i64().unwrap();
        let fetched = crate::services::research_notes::get_note(&handler.db, note_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.price_at_write, Some(166.48));
        assert_eq!(fetched.invalidation_price, Some(156.0));
        assert_eq!(
            fetched.invalidation_kind,
            Some(crate::services::research_notes::InvalidationKind::CloseBelow)
        );
        assert_eq!(fetched.targets.len(), 2);
        assert_eq!(fetched.targets[1].label, "T2");
    }

    #[tokio::test]
    async fn write_research_note_invalid_invalidation_kind_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                invalidation_price: Some(150.0),
                invalidation_kind: Some("dunno".into()),
                ..args("TSLA", "x")
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_research_note_kind_required_when_price_set() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                invalidation_price: Some(150.0),
                invalidation_kind: None,
                ..args("TSLA", "x")
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_research_note_caps_targets_at_four() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let too_many = (0..5)
            .map(|i| WriteResearchNoteTarget {
                label: format!("T{}", i + 1),
                price: 100.0 + i as f64,
            })
            .collect();
        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                targets: Some(too_many),
                ..args("TSLA", "x")
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    /// When the author omits `price_at_write`, the tool should
    /// snapshot the live quote and store its `last_price`. This
    /// exercises the full path through the connected `MockIbkrClient`
    /// (which returns canned `last_price = 150.35`).
    #[tokio::test]
    async fn write_research_note_auto_snapshots_price_when_omitted() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;

        let r = handler
            .write_research_note(Parameters(args("AAPL", "no price provided")))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let note_id = r.structured_content.unwrap()["note_id"].as_i64().unwrap();
        let fetched = crate::services::research_notes::get_note(&handler.db, note_id)
            .await
            .unwrap()
            .unwrap();
        // MockIbkrClient::get_market_data_snapshot returns last_price = 150.35.
        assert_eq!(fetched.price_at_write, Some(150.35));
    }

    /// When the author *does* pass `price_at_write`, the tool must not
    /// overwrite it with a snapshot.
    #[tokio::test]
    async fn write_research_note_explicit_price_wins_over_snapshot() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        let handler = handler_for_mock_ibkr(db, mock).await;

        let r = handler
            .write_research_note(Parameters(WriteResearchNoteArgs {
                price_at_write: Some(99.99),
                ..args("AAPL", "explicit price")
            }))
            .await
            .expect("ok");
        let note_id = r.structured_content.unwrap()["note_id"].as_i64().unwrap();
        let fetched = crate::services::research_notes::get_note(&handler.db, note_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.price_at_write, Some(99.99));
    }

    /// When the live-quote path errors (TWS disconnected, in this case
    /// because the mock starts disconnected via `handler_for_db` which
    /// uses `MockIbkrClient::new` without `set_connected(true)`), the
    /// write must still succeed with `price_at_write = None`.
    #[tokio::test]
    async fn write_research_note_swallows_quote_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db); // disconnected mock
        let r = handler
            .write_research_note(Parameters(args("AAPL", "ibkr offline")))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let note_id = r.structured_content.unwrap()["note_id"].as_i64().unwrap();
        let fetched = crate::services::research_notes::get_note(&handler.db, note_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.price_at_write, None);
    }
}
