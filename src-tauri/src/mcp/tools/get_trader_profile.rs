//! `get_trader_profile` — Phase 6 read tool.
//!
//! Returns the trader's behavioral profile aggregated over the trailing
//! `window_days` (default 30) of `day_reviews` for the resolved
//! account. Pure SQL aggregator — no LLM, no IBKR, no audit row.
//! Consumed by `agent/morning_sweep.py` to condition tomorrow's
//! playbook on the trader's recent behavioral history.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::{map_tool_result, resolve_account};
use crate::services::trader_profile;

const DEFAULT_WINDOW_DAYS: u32 = 30;
const MIN_WINDOW_DAYS: u32 = 1;
const MAX_WINDOW_DAYS: u32 = 365;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTraderProfileArgs {
    /// Trailing-day window over `day_reviews`. Defaults to 30; clamped
    /// to [1, 365].
    #[serde(default)]
    pub window_days: Option<u32>,
    /// Optional account; defaults to the sole managed account when omitted.
    #[serde(default)]
    pub account: Option<String>,
}

#[tool_router(router = get_trader_profile_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_trader_profile",
        description = "Aggregate the trader's behavioral history from `day_reviews` over the trailing `window_days` (default 30, clamped to [1, 365]). Optional `account` (defaults to the sole managed account). Returns `{ account, window_days, since_date, n_reviews, tag_frequencies[], pnl_by_tag[], trendline: { last_7d, prior_21d }, recent_incidents[] }`. Pure SQL aggregate over `day_reviews` — no LLM cost, no audit row. `n_reviews: 0` when no reviews exist for the window."
    )]
    pub async fn get_trader_profile(
        &self,
        Parameters(args): Parameters<GetTraderProfileArgs>,
    ) -> Result<CallToolResult, McpError> {
        let account =
            match resolve_account(self.ibkr_client.as_ref(), args.account.as_deref()).await {
                Ok(a) => a,
                Err(e) => return map_tool_result::<(), String>(Err(e)),
            };

        let window_days = args
            .window_days
            .unwrap_or(DEFAULT_WINDOW_DAYS)
            .clamp(MIN_WINDOW_DAYS, MAX_WINDOW_DAYS);

        match trader_profile::aggregate(&self.db, &account, window_days).await {
            Ok(profile) => map_tool_result::<_, String>(Ok(profile)),
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
    use chrono::{Duration, NaiveDate, Utc};
    use chrono_tz::America::New_York;
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

    fn write_request(
        date: NaiveDate,
        account: &str,
        tags: Vec<BehavioralTag>,
        net_pnl: f64,
    ) -> WriteTradeReviewRequest {
        WriteTradeReviewRequest {
            date,
            account: account.into(),
            prompt_version: 1,
            summary: LegSummary {
                gross_pnl: net_pnl,
                net_pnl,
                commissions_total: 0.0,
                n_round_trips: 1,
                n_carryover: 0,
                win_rate: Some(1.0),
                by_symbol: Default::default(),
            },
            behavioral_tags: tags,
            leg_observations: vec![],
            narrative_md: "x".into(),
            llm_call_id: None,
        }
    }

    #[tokio::test]
    async fn get_trader_profile_empty_returns_zero_review_envelope() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let r = handler
            .get_trader_profile(Parameters(GetTraderProfileArgs {
                window_days: Some(30),
                account: Some("U1".into()),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["account"], "U1");
        assert_eq!(body["window_days"], 30);
        assert_eq!(body["n_reviews"], 0);
        assert!(body["tag_frequencies"].as_array().unwrap().is_empty());
        assert!(body["recent_incidents"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_trader_profile_returns_aggregated_profile() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let store = TradeReviewStore::new(handler.db.clone());
        let today = Utc::now().with_timezone(&New_York).date_naive();
        store
            .write(
                write_request(
                    today - Duration::days(1),
                    "U1",
                    vec![BehavioralTag::FlatClose, BehavioralTag::ChaseOwnExit],
                    100.0,
                ),
                crate::services::trade_reviews::ReviewV2Fields::v1_only(),
            )
            .await
            .unwrap();
        store
            .write(
                write_request(
                    today - Duration::days(2),
                    "U1",
                    vec![BehavioralTag::FlatClose],
                    200.0,
                ),
                crate::services::trade_reviews::ReviewV2Fields::v1_only(),
            )
            .await
            .unwrap();

        let r = handler
            .get_trader_profile(Parameters(GetTraderProfileArgs {
                window_days: Some(30),
                account: Some("U1".into()),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["n_reviews"], 2);
        let tags = body["tag_frequencies"].as_array().expect("array");
        assert_eq!(tags.len(), 2);
        // Sorted by count desc — flat_close (2) before chase_own_exit (1).
        assert_eq!(tags[0]["tag"], "flat_close");
        assert_eq!(tags[0]["count"], 2);
    }

    #[tokio::test]
    async fn get_trader_profile_defaults_window_when_omitted() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let r = handler
            .get_trader_profile(Parameters(GetTraderProfileArgs {
                window_days: None,
                account: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["window_days"], 30);
        assert_eq!(body["account"], "U1");
    }

    #[tokio::test]
    async fn get_trader_profile_clamps_window() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let r = handler
            .get_trader_profile(Parameters(GetTraderProfileArgs {
                window_days: Some(0),
                account: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["window_days"], 1);

        let r = handler
            .get_trader_profile(Parameters(GetTraderProfileArgs {
                window_days: Some(10_000),
                account: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["window_days"], 365);
    }

    #[tokio::test]
    async fn get_trader_profile_writes_no_audit_row() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let _ = handler
            .get_trader_profile(Parameters(GetTraderProfileArgs {
                window_days: None,
                account: None,
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

    #[tokio::test]
    async fn get_trader_profile_multi_account_errors_when_unspecified() {
        let (_tmp, db) = make_db();
        let mock = Arc::new(MockIbkrClient::new());
        mock.set_accounts(vec!["U1".into(), "U2".into()]).await;
        let handler = handler_for_mock_ibkr(db, mock).await;

        let r = handler
            .get_trader_profile(Parameters(GetTraderProfileArgs {
                window_days: None,
                account: None,
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
