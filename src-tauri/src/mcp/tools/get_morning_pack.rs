//! `get_morning_pack` — Phase 7 read tool.
//!
//! Returns the agent-authored morning pack for the requested
//! `YYYY-MM-DD` ET trading day. The pack is the source of truth for
//! "yesterday's predictions" the EOD review scores against, so the
//! tool surfaces the full set of ranked ideas verbatim — including
//! the `entry_zone` / `invalidation` strings the outcome extractor
//! parses.

use chrono::NaiveDate;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::agent_morning_packs;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetMorningPackArgs {
    /// `YYYY-MM-DD` (ET trading day) the pack was written for.
    pub date: String,
}

#[tool_router(router = get_morning_pack_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_morning_pack",
        description = "Return the agent-authored morning pack for `date` (`YYYY-MM-DD`, ET). Returns `{ date, written_by, written_at, ranked_ideas: [{symbol, thesis_md, conviction, entry_zone, invalidation, evidence_refs}, ...] }`, or `{ date, ranked_ideas: [] }` if no pack was written for that date. Use this to recall yesterday's predictions before scoring outcomes."
    )]
    pub async fn get_morning_pack(
        &self,
        Parameters(args): Parameters<GetMorningPackArgs>,
    ) -> Result<CallToolResult, McpError> {
        let date = match NaiveDate::parse_from_str(&args.date, "%Y-%m-%d") {
            Ok(d) => d,
            Err(e) => {
                return map_tool_result::<(), String>(Err(format!("date must be YYYY-MM-DD: {e}")));
            }
        };

        match agent_morning_packs::get_pack(&self.db, date).await {
            Ok(Some(pack)) => map_tool_result::<_, String>(Ok(json!({
                "date": pack.date.to_string(),
                "written_by": pack.written_by,
                "written_at": pack.written_at.timestamp(),
                "ranked_ideas": pack.ranked_ideas,
            }))),
            Ok(None) => map_tool_result::<_, String>(Ok(json!({
                "date": date.to_string(),
                "written_by": null,
                "written_at": null,
                "ranked_ideas": [],
            }))),
            Err(e) => map_tool_result::<(), String>(Err(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use crate::services::agent_morning_packs::{self, NewAgentMorningPack, RankedIdea};
    use crate::services::research_notes::Conviction;

    fn idea(symbol: &str) -> RankedIdea {
        RankedIdea {
            symbol: symbol.to_string(),
            thesis_md: "looks bullish".into(),
            conviction: Some(Conviction::A),
            entry_zone: Some("100-105".into()),
            invalidation: Some("close < 95".into()),
            evidence_refs: Vec::new(),
        }
    }

    #[tokio::test]
    async fn get_morning_pack_returns_persisted_pack() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let d = NaiveDate::parse_from_str("2026-05-02", "%Y-%m-%d").unwrap();
        agent_morning_packs::write_pack(
            &handler.db,
            NewAgentMorningPack {
                date: d,
                ranked_ideas: vec![idea("TSLA"), idea("AAPL")],
                written_by: "agent_morning_sweep".into(),
            },
        )
        .await
        .unwrap();

        let r = handler
            .get_morning_pack(Parameters(GetMorningPackArgs {
                date: "2026-05-02".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["date"], "2026-05-02");
        assert_eq!(body["written_by"], "agent_morning_sweep");
        let arr = body["ranked_ideas"].as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["symbol"], "TSLA");
    }

    #[tokio::test]
    async fn get_morning_pack_absent_returns_empty_ideas() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_morning_pack(Parameters(GetMorningPackArgs {
                date: "2026-05-02".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["date"], "2026-05-02");
        assert!(body["ranked_ideas"].as_array().unwrap().is_empty());
        assert!(body["written_by"].is_null());
    }

    #[tokio::test]
    async fn get_morning_pack_invalid_date_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_morning_pack(Parameters(GetMorningPackArgs {
                date: "garbage".into(),
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
