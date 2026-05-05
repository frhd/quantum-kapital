//! `get_trade_review` — Phase 4 read tool.
//!
//! Returns the structured trade review for `(date, account, prompt_version?)`.
//! When `prompt_version` is omitted, the latest version for the day is returned
//! so the agent and UI default to the freshest grade. Empty days return a
//! `{date, review: null}` envelope rather than an error — same convention as
//! `get_morning_pack`.

use chrono::NaiveDate;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::trade_reviews::TradeReviewStore;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTradeReviewArgs {
    /// `YYYY-MM-DD` ET trading day the review was written for.
    pub date: String,
    /// Optional account; defaults to the sole managed account when omitted.
    #[serde(default)]
    pub account: Option<String>,
    /// Optional rubric/prompt version. Omit for the latest version on `date`.
    #[serde(default)]
    pub prompt_version: Option<i32>,
}

#[tool_router(router = get_trade_review_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_trade_review",
        description = "Return the structured trade review for `date` (`YYYY-MM-DD`, ET). Optional `account` (defaults to the sole managed account) and `prompt_version` (defaults to the latest version on `date`). Returns `{ date, account, prompt_version, review: { formula_version, generated_at, score_v2?, discipline_v2?, risk_metrics?, equity_curve?, grade?, grade_score?, summary, behavioral_tags[], leg_observations[], narrative_md, llm_call_id } | null }`. `formula_version` (`v1` for pre-Phase-4 rows, `v2` for new writes) tells you which scoring fields are populated — pre-P4 rows carry the legacy `(grade, grade_score)` and the v2 numerics are NULL; post-P4 rows carry `(score_v2, discipline_v2, risk_metrics, equity_curve)` and the legacy fields are NULL. Never sum `score_v2` and `discipline_v2` for ranking. The `review` is null when no row was written for the requested key — not an error."
    )]
    pub async fn get_trade_review(
        &self,
        Parameters(args): Parameters<GetTradeReviewArgs>,
    ) -> Result<CallToolResult, McpError> {
        let date = match NaiveDate::parse_from_str(&args.date, "%Y-%m-%d") {
            Ok(d) => d,
            Err(e) => {
                return map_tool_result::<(), String>(Err(format!("date must be YYYY-MM-DD: {e}")));
            }
        };

        let account = match crate::mcp::tools::resolve_account(
            self.ibkr_client.as_ref(),
            args.account.as_deref(),
        )
        .await
        {
            Ok(a) => a,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let store = TradeReviewStore::new(self.db.clone());
        let outcome = match args.prompt_version {
            Some(v) => store.read(date, &account, v).await,
            None => store.read_latest(date, &account).await,
        };

        match outcome {
            Ok(Some(review)) => map_tool_result::<_, String>(Ok(json!({
                "date": review.date.to_string(),
                "account": review.account,
                "prompt_version": review.prompt_version,
                "review": review,
            }))),
            Ok(None) => map_tool_result::<_, String>(Ok(json!({
                "date": date.to_string(),
                "account": account,
                "prompt_version": args.prompt_version,
                "review": null,
            }))),
            Err(e) => map_tool_result::<(), String>(Err(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::mocks::MockIbkrClient;
    use crate::mcp::tools::test_support::{handler_for_mock_ibkr, make_db};
    use crate::services::trade_reviews::{
        BehavioralTag, LegSummary, TradeReviewStore, WriteTradeReviewRequest,
    };
    use std::sync::Arc;

    async fn handler_with_account(
        account: &str,
    ) -> (tempfile::NamedTempFile, crate::mcp::handler::McpHandler) {
        let (tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec![account.to_string()]).await;
        let handler = handler_for_mock_ibkr(db, mock).await;
        (tmp, handler)
    }

    fn sample_request(
        date: NaiveDate,
        account: &str,
        prompt_version: i32,
    ) -> WriteTradeReviewRequest {
        WriteTradeReviewRequest {
            date,
            account: account.into(),
            prompt_version,
            summary: LegSummary {
                gross_pnl: 100.0,
                net_pnl: 90.0,
                commissions_total: 10.0,
                n_round_trips: 1,
                n_carryover: 0,
                win_rate: Some(1.0),
                by_symbol: Default::default(),
            },
            behavioral_tags: vec![BehavioralTag::FlatClose],
            leg_observations: vec![],
            narrative_md: format!("v{prompt_version}"),
            llm_call_id: None,
        }
    }

    #[tokio::test]
    async fn get_trade_review_returns_persisted_row() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let store = TradeReviewStore::new(handler.db.clone());
        let date = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        store
            .write(
                sample_request(date, "U1", 1),
                crate::services::trade_reviews::ReviewV2Fields::v1_only(),
            )
            .await
            .unwrap();

        let r = handler
            .get_trade_review(Parameters(GetTradeReviewArgs {
                date: "2026-05-04".into(),
                account: Some("U1".into()),
                prompt_version: Some(1),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["date"], "2026-05-04");
        assert_eq!(body["account"], "U1");
        assert_eq!(body["prompt_version"], 1);
        let review = &body["review"];
        assert!(review.is_object(), "review must be object: {body}");
        assert_eq!(review["narrative_md"], "v1");
    }

    #[tokio::test]
    async fn get_trade_review_absent_returns_null_envelope() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let r = handler
            .get_trade_review(Parameters(GetTradeReviewArgs {
                date: "2026-05-04".into(),
                account: Some("U1".into()),
                prompt_version: None,
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert!(body["review"].is_null());
    }

    #[tokio::test]
    async fn get_trade_review_invalid_date_errors() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let r = handler
            .get_trade_review(Parameters(GetTradeReviewArgs {
                date: "garbage".into(),
                account: Some("U1".into()),
                prompt_version: None,
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn get_trade_review_defaults_to_latest_prompt_version() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let store = TradeReviewStore::new(handler.db.clone());
        let date = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        store
            .write(
                sample_request(date, "U1", 1),
                crate::services::trade_reviews::ReviewV2Fields::v1_only(),
            )
            .await
            .unwrap();
        store
            .write(
                sample_request(date, "U1", 5),
                crate::services::trade_reviews::ReviewV2Fields::v1_only(),
            )
            .await
            .unwrap();
        store
            .write(
                sample_request(date, "U1", 3),
                crate::services::trade_reviews::ReviewV2Fields::v1_only(),
            )
            .await
            .unwrap();

        let r = handler
            .get_trade_review(Parameters(GetTradeReviewArgs {
                date: "2026-05-04".into(),
                account: Some("U1".into()),
                prompt_version: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["prompt_version"], 5);
        assert_eq!(body["review"]["prompt_version"], 5);
        assert_eq!(body["review"]["narrative_md"], "v5");
    }

    #[tokio::test]
    async fn get_trade_review_writes_no_audit_row() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let store = TradeReviewStore::new(handler.db.clone());
        let date = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        store
            .write(
                sample_request(date, "U1", 1),
                crate::services::trade_reviews::ReviewV2Fields::v1_only(),
            )
            .await
            .unwrap();

        let _ = handler
            .get_trade_review(Parameters(GetTradeReviewArgs {
                date: "2026-05-04".into(),
                account: None,
                prompt_version: None,
            }))
            .await
            .expect("ok");

        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert!(
            audits.is_empty(),
            "read tool must not write audit rows; got {audits:?}"
        );
    }
}
