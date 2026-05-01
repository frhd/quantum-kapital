//! `write_morning_pack` — agent-authored ranked-ideas pack.
//!
//! Wraps `services::agent_morning_packs::write_pack`. Idempotent on
//! `date` — a re-run of the morning sweep overwrites cleanly. Distinct
//! from the deterministic `morning_packs` table the EOD ranker writes
//! to.

use chrono::NaiveDate;
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
use crate::services::agent_morning_packs::{self, NewAgentMorningPack, RankedIdea};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteMorningPackArgs {
    /// `YYYY-MM-DD` ET trading-day date the pack applies to.
    pub date: String,
    /// Ordered list of ranked ideas. Order is the rank — top idea first.
    pub ranked_ideas: Vec<serde_json::Value>,
}

#[tool_router(router = write_morning_pack_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "write_morning_pack",
        description = "Persist a ranked-ideas morning pack for `date` (`YYYY-MM-DD`, ET). Idempotent: a second call for the same date overwrites cleanly with no duplicate rows. Each idea must contain `symbol` and `thesis_md`; `conviction` (A|B|C), `entry_zone`, `invalidation`, and `evidence_refs` (alert | news | setup | bar_range) are optional. Returns `{ date, idea_count, written_at }`."
    )]
    pub async fn write_morning_pack(
        &self,
        Parameters(args): Parameters<WriteMorningPackArgs>,
    ) -> Result<CallToolResult, McpError> {
        let date = match NaiveDate::parse_from_str(&args.date, "%Y-%m-%d") {
            Ok(d) => d,
            Err(e) => {
                return map_tool_result::<(), String>(Err(format!("date must be YYYY-MM-DD: {e}")));
            }
        };
        if args.ranked_ideas.is_empty() {
            return map_tool_result::<(), String>(Err(
                "ranked_ideas must contain at least one entry".to_string(),
            ));
        }
        let mut ideas: Vec<RankedIdea> = Vec::with_capacity(args.ranked_ideas.len());
        for (i, raw) in args.ranked_ideas.iter().enumerate() {
            match serde_json::from_value::<RankedIdea>(raw.clone()) {
                Ok(idea) => ideas.push(idea),
                Err(e) => {
                    return map_tool_result::<(), String>(Err(format!(
                        "ranked_ideas[{i}] invalid: {e}"
                    )));
                }
            }
        }

        let input_for_audit = json!({
            "date": args.date,
            "idea_count": ideas.len(),
        });
        let audit_id = match record_audit(
            &self.db,
            "write_morning_pack",
            &input_for_audit,
            &self.caller,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let outcome = agent_morning_packs::write_pack(
            &self.db,
            NewAgentMorningPack {
                date,
                ranked_ideas: ideas,
                written_by: self.caller.clone(),
            },
        )
        .await;

        match outcome {
            Ok(saved) => {
                stamp_audit_summary(
                    &self.db,
                    audit_id,
                    &format!("agent_morning_packs.date={}", saved.date),
                )
                .await;
                emit_event(
                    &self.emitter,
                    AppEvent::AgentMorningPackWritten {
                        date: saved.date,
                        idea_count: saved.ranked_ideas.len(),
                    },
                )
                .await;
                map_tool_result::<_, String>(Ok(json!({
                    "date": saved.date.to_string(),
                    "idea_count": saved.ranked_ideas.len(),
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

    fn idea(symbol: &str) -> serde_json::Value {
        serde_json::json!({
            "symbol": symbol,
            "thesis_md": "looks bullish",
            "conviction": "B",
            "entry_zone": "100-105",
            "invalidation": "close < 95",
        })
    }

    #[tokio::test]
    async fn write_morning_pack_persists_with_audit_and_event() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let r = handler
            .write_morning_pack(Parameters(WriteMorningPackArgs {
                date: "2026-05-04".into(),
                ranked_ideas: vec![idea("TSLA"), idea("AAPL")],
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["idea_count"].as_u64().unwrap(), 2);

        // Persisted with both ideas.
        let fetched = crate::services::agent_morning_packs::get_pack(
            &handler.db,
            chrono::NaiveDate::parse_from_str("2026-05-04", "%Y-%m-%d").unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(fetched.ranked_ideas.len(), 2);

        // Audit row.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].tool, "write_morning_pack");
    }

    #[tokio::test]
    async fn write_morning_pack_is_idempotent_on_date() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        for ideas in [vec![idea("TSLA")], vec![idea("AAPL"), idea("MSFT")]] {
            let r = handler
                .write_morning_pack(Parameters(WriteMorningPackArgs {
                    date: "2026-05-05".into(),
                    ranked_ideas: ideas,
                }))
                .await
                .expect("ok");
            assert_eq!(r.is_error, Some(false));
        }

        let fetched = crate::services::agent_morning_packs::get_pack(
            &handler.db,
            chrono::NaiveDate::parse_from_str("2026-05-05", "%Y-%m-%d").unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(fetched.ranked_ideas.len(), 2);
        let symbols: Vec<&str> = fetched
            .ranked_ideas
            .iter()
            .map(|i| i.symbol.as_str())
            .collect();
        assert_eq!(symbols, vec!["AAPL", "MSFT"]);
    }

    #[tokio::test]
    async fn write_morning_pack_invalid_date_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .write_morning_pack(Parameters(WriteMorningPackArgs {
                date: "not-a-date".into(),
                ranked_ideas: vec![idea("TSLA")],
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_morning_pack_empty_ideas_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .write_morning_pack(Parameters(WriteMorningPackArgs {
                date: "2026-05-04".into(),
                ranked_ideas: vec![],
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
