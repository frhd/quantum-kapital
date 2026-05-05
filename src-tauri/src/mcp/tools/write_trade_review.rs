//! `write_trade_review` — agent-write rail for the structured trade review.
//!
//! Mirrors `write_morning_pack`: audited via `mcp_audit`, emits an
//! `AppEvent` on success, idempotent on `(date, account, prompt_version)`.
//! The grade is computed server-side from `(summary, behavioral_tags)`
//! — the agent never picks the grade.

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
use crate::services::trade_reviews::{
    BehavioralTag, LegObservation, LegSummary, TradeReviewStore, WriteTradeReviewRequest,
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteTradeReviewArgs {
    /// `YYYY-MM-DD` ET trading day the review covers.
    pub date: String,
    /// Account the review applies to.
    pub account: String,
    /// Rubric/prompt version. Bump when the rubric, tag enum, or system
    /// prompt changes materially.
    pub prompt_version: i32,
    /// Server-side numerical summary of the day's legs. Computed by the
    /// agent from `get_trade_legs(date)` and forwarded verbatim.
    pub summary: LegSummary,
    /// Picked from the closed `BehavioralTag` enum. Empty list is allowed
    /// (a flat, unremarkable day with no positive or negative tags).
    #[serde(default)]
    pub behavioral_tags: Vec<BehavioralTag>,
    /// Optional per-leg notes. Each `tag` (when present) must also appear
    /// in `behavioral_tags`.
    #[serde(default)]
    pub leg_observations: Vec<LegObservation>,
    /// Markdown narrative the LLM authored. Required, non-empty.
    pub narrative_md: String,
    /// Optional pointer to the originating `llm_calls.id` row.
    #[serde(default)]
    pub llm_call_id: Option<String>,
}

#[tool_router(router = write_trade_review_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "write_trade_review",
        description = "Persist a structured trade review for `date` (`YYYY-MM-DD`, ET) + `account` + `prompt_version`. The server computes the grade deterministically from `(summary, behavioral_tags)` — never pass a grade. `behavioral_tags` are picked from a closed enum (chase_own_exit, late_otm_lottery, gamma_window_violation, single_name_concentration, position_sizing_ungraduated, post_loss_revenge, flat_close, discipline_on_loser, scaled_in_winner, scaled_in_loser, thesis_match_executed, off_thesis_trade). Idempotent on `(date, account, prompt_version)` — a re-run overwrites cleanly. Returns `{ date, account, prompt_version, grade, score, generated_at }`."
    )]
    pub async fn write_trade_review(
        &self,
        Parameters(args): Parameters<WriteTradeReviewArgs>,
    ) -> Result<CallToolResult, McpError> {
        let date = match chrono::NaiveDate::parse_from_str(&args.date, "%Y-%m-%d") {
            Ok(d) => d,
            Err(e) => {
                return map_tool_result::<(), String>(Err(format!("date must be YYYY-MM-DD: {e}")));
            }
        };
        if args.account.trim().is_empty() {
            return map_tool_result::<(), String>(Err("account must be non-empty".into()));
        }
        if args.narrative_md.trim().is_empty() {
            return map_tool_result::<(), String>(Err("narrative_md must be non-empty".into()));
        }

        let input_for_audit = json!({
            "date": args.date,
            "account": args.account,
            "prompt_version": args.prompt_version,
            "tag_count": args.behavioral_tags.len(),
            "obs_count": args.leg_observations.len(),
        });
        let audit_id = match record_audit(
            &self.db,
            "write_trade_review",
            &input_for_audit,
            &self.caller,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let store = TradeReviewStore::new(self.db.clone());
        let req = WriteTradeReviewRequest {
            date,
            account: args.account.clone(),
            prompt_version: args.prompt_version,
            summary: args.summary,
            behavioral_tags: args.behavioral_tags,
            leg_observations: args.leg_observations,
            narrative_md: args.narrative_md,
            llm_call_id: args.llm_call_id,
        };

        match store.write(req).await {
            Ok(outcome) => {
                stamp_audit_summary(
                    &self.db,
                    audit_id,
                    &format!(
                        "day_reviews date={} account={} prompt_version={} grade={}",
                        outcome.review.date,
                        outcome.review.account,
                        outcome.review.prompt_version,
                        outcome.grade.grade.as_str(),
                    ),
                )
                .await;
                emit_event(
                    &self.emitter,
                    AppEvent::TradeReviewWritten {
                        date: outcome.review.date,
                        account: outcome.review.account.clone(),
                        prompt_version: outcome.review.prompt_version,
                        grade: outcome.grade.grade.as_str().to_string(),
                    },
                )
                .await;
                map_tool_result::<_, String>(Ok(json!({
                    "date": outcome.review.date.to_string(),
                    "account": outcome.review.account,
                    "prompt_version": outcome.review.prompt_version,
                    "grade": outcome.grade.grade.as_str(),
                    "score": outcome.grade.score,
                    "generated_at": outcome.review.generated_at,
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
    use crate::services::trade_reviews::{BehavioralTag, LegSummary, TradeReviewStore};

    fn good_args(prompt_version: i32) -> WriteTradeReviewArgs {
        WriteTradeReviewArgs {
            date: "2026-05-04".into(),
            account: "U1234567".into(),
            prompt_version,
            summary: LegSummary {
                gross_pnl: 401.10,
                net_pnl: 380.0,
                commissions_total: 21.10,
                n_round_trips: 3,
                n_carryover: 0,
                win_rate: Some(2.0 / 3.0),
                by_symbol: Default::default(),
            },
            behavioral_tags: vec![BehavioralTag::FlatClose, BehavioralTag::DisciplineOnLoser],
            leg_observations: vec![],
            narrative_md: "A solid disciplined day.".into(),
            llm_call_id: Some("llm-call-7".into()),
        }
    }

    #[tokio::test]
    async fn write_trade_review_persists_with_audit_and_event() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .write_trade_review(Parameters(good_args(1)))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["date"], "2026-05-04");
        assert_eq!(body["account"], "U1234567");
        assert_eq!(body["prompt_version"], 1);
        let grade = body["grade"].as_str().unwrap();
        assert!(["A", "B", "C", "D", "F"].contains(&grade));

        // Stored.
        let store = TradeReviewStore::new(handler.db.clone());
        let row = store
            .read(
                chrono::NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(),
                "U1234567",
                1,
            )
            .await
            .unwrap()
            .expect("row");
        assert_eq!(row.narrative_md, "A solid disciplined day.");

        // Audit row.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].tool, "write_trade_review");
    }

    #[tokio::test]
    async fn write_trade_review_is_idempotent() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        for narrative in ["first", "second"] {
            let mut args = good_args(1);
            args.narrative_md = narrative.into();
            let r = handler
                .write_trade_review(Parameters(args))
                .await
                .expect("ok");
            assert_eq!(r.is_error, Some(false), "narrative={narrative}");
        }

        let store = TradeReviewStore::new(handler.db.clone());
        assert_eq!(store.count().await.unwrap(), 1);
        let row = store
            .read(
                chrono::NaiveDate::from_ymd_opt(2026, 5, 4).unwrap(),
                "U1234567",
                1,
            )
            .await
            .unwrap()
            .expect("row");
        assert_eq!(row.narrative_md, "second");

        // Each call writes its own audit row (mirrors `write_morning_pack`).
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 2);
    }

    #[tokio::test]
    async fn write_trade_review_invalid_date_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let mut args = good_args(1);
        args.date = "garbage".into();
        let r = handler
            .write_trade_review(Parameters(args))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_trade_review_empty_account_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let mut args = good_args(1);
        args.account = "  ".into();
        let r = handler
            .write_trade_review(Parameters(args))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_trade_review_empty_narrative_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let mut args = good_args(1);
        args.narrative_md = "".into();
        let r = handler
            .write_trade_review(Parameters(args))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_trade_review_grade_is_deterministic() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r1 = handler
            .write_trade_review(Parameters(good_args(1)))
            .await
            .expect("ok");
        let g1 = r1.structured_content.expect("structured")["grade"].clone();

        let r2 = handler
            .write_trade_review(Parameters(good_args(2))) // bumped prompt_version
            .await
            .expect("ok");
        let g2 = r2.structured_content.expect("structured")["grade"].clone();
        assert_eq!(g1, g2, "same inputs → same grade across prompt_versions");
    }
}
