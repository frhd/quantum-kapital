//! `get_today_playbook` — Phase 5 read tool.
//!
//! Returns the structured playbook for `(date, account, generation_id?)`.
//! When `generation_id` is omitted, the latest generation for the day is
//! returned so the agent and UI default to the freshest playbook. Empty
//! days return a `{date, playbook: null}` envelope rather than an error
//! — same convention as `get_morning_pack` and `get_trade_review`.

use chrono::NaiveDate;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::playbooks::PlaybookStore;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTodayPlaybookArgs {
    /// `YYYY-MM-DD` ET trading day the playbook was written for.
    pub date: String,
    /// Optional account; defaults to the sole managed account when omitted.
    #[serde(default)]
    pub account: Option<String>,
    /// Optional explicit generation. Omit for the latest generation on `date`.
    #[serde(default)]
    pub generation_id: Option<i32>,
}

#[tool_router(router = get_today_playbook_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_today_playbook",
        description = "Return the structured pre-market playbook for `date` (`YYYY-MM-DD`, ET). Optional `account` (defaults to the sole managed account) and `generation_id` (defaults to the latest generation on `date`). Returns `{ date, account, generation_id, playbook: { generated_at, ranked_setups: [{symbol, bias, trigger, entry, invalidation, target_1, target_2?, conviction, rationale_md, evidence_refs}], skip_list: [{symbol, reason}], llm_call_id } | null }`. The `playbook` is null when no row was written for the requested key — not an error."
    )]
    pub async fn get_today_playbook(
        &self,
        Parameters(args): Parameters<GetTodayPlaybookArgs>,
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

        let store = PlaybookStore::new(self.db.clone());
        let outcome = match args.generation_id {
            Some(g) => store.read_generation(date, &account, g).await,
            None => store.read_latest(date, &account).await,
        };

        match outcome {
            Ok(Some(pb)) => map_tool_result::<_, String>(Ok(json!({
                "date": pb.date.to_string(),
                "account": pb.account,
                "generation_id": pb.generation_id,
                "playbook": pb,
            }))),
            Ok(None) => map_tool_result::<_, String>(Ok(json!({
                "date": date.to_string(),
                "account": account,
                "generation_id": args.generation_id,
                "playbook": null,
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
    use crate::services::playbooks::{
        Conviction, PlaybookStore, RankedSetup, SetupBias, SkipEntry, WritePlaybookRequest,
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

    fn sample_request(date: NaiveDate, account: &str, label: &str) -> WritePlaybookRequest {
        WritePlaybookRequest {
            date,
            account: account.into(),
            ranked_setups: vec![RankedSetup {
                symbol: format!("SYM_{label}"),
                bias: SetupBias::Long,
                trigger: "trigger".into(),
                entry: "100".into(),
                invalidation: "lose 95".into(),
                target_1: "110".into(),
                target_2: None,
                conviction: Conviction::B,
                rationale_md: label.into(),
                evidence_refs: vec![],
            }],
            skip_list: vec![SkipEntry {
                symbol: "SKIP".into(),
                reason: "no edge".into(),
            }],
            llm_call_id: None,
        }
    }

    #[tokio::test]
    async fn get_today_playbook_returns_persisted_row() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let store = PlaybookStore::new(handler.db.clone());
        let date = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
        store.write(sample_request(date, "U1", "g1")).await.unwrap();

        let r = handler
            .get_today_playbook(Parameters(GetTodayPlaybookArgs {
                date: "2026-05-05".into(),
                account: Some("U1".into()),
                generation_id: Some(1),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["date"], "2026-05-05");
        assert_eq!(body["account"], "U1");
        assert_eq!(body["generation_id"], 1);
        let pb = &body["playbook"];
        assert!(pb.is_object(), "playbook must be object: {body}");
        assert_eq!(pb["ranked_setups"][0]["symbol"], "SYM_g1");
    }

    #[tokio::test]
    async fn get_today_playbook_absent_returns_null_envelope() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let r = handler
            .get_today_playbook(Parameters(GetTodayPlaybookArgs {
                date: "2026-05-05".into(),
                account: Some("U1".into()),
                generation_id: None,
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert!(body["playbook"].is_null());
    }

    #[tokio::test]
    async fn get_today_playbook_invalid_date_errors() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let r = handler
            .get_today_playbook(Parameters(GetTodayPlaybookArgs {
                date: "garbage".into(),
                account: Some("U1".into()),
                generation_id: None,
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn get_today_playbook_defaults_to_latest_generation() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let store = PlaybookStore::new(handler.db.clone());
        let date = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
        store.write(sample_request(date, "U1", "g1")).await.unwrap();
        store.write(sample_request(date, "U1", "g2")).await.unwrap();
        store.write(sample_request(date, "U1", "g3")).await.unwrap();

        let r = handler
            .get_today_playbook(Parameters(GetTodayPlaybookArgs {
                date: "2026-05-05".into(),
                account: Some("U1".into()),
                generation_id: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["generation_id"], 3);
        assert_eq!(body["playbook"]["generation_id"], 3);
        assert_eq!(body["playbook"]["ranked_setups"][0]["symbol"], "SYM_g3");
    }

    #[tokio::test]
    async fn get_today_playbook_writes_no_audit_row() {
        let (_tmp, handler) = handler_with_account("U1").await;
        let store = PlaybookStore::new(handler.db.clone());
        let date = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
        store.write(sample_request(date, "U1", "g1")).await.unwrap();

        let _ = handler
            .get_today_playbook(Parameters(GetTodayPlaybookArgs {
                date: "2026-05-05".into(),
                account: None,
                generation_id: None,
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
