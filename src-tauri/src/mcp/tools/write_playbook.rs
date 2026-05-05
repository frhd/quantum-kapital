//! `write_playbook` — agent-write rail for the structured pre-market playbook.
//!
//! Mirrors `write_morning_pack` / `write_trade_review`: audited via
//! `mcp_audit`, emits an `AppEvent` on success. The store assigns
//! `generation_id` (next-after-MAX per `(date, account)`), so callers
//! never pass it. v1 writes one playbook per cron tick; an intraday
//! refresh hook can be added later without migration.

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
use crate::services::playbooks::{
    PlaybookStore, RankedSetup, SkipEntry, WritePlaybookRequest,
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WritePlaybookArgs {
    /// `YYYY-MM-DD` ET trading day the playbook covers.
    pub date: String,
    /// Account the playbook applies to.
    pub account: String,
    /// Ranked, actionable setups. Empty list is allowed (a no-trade day);
    /// in that case `skip_list` should explain why.
    #[serde(default)]
    pub ranked_setups: Vec<RankedSetup>,
    /// Symbols explicitly excluded today, with reasons. Empty allowed.
    #[serde(default)]
    pub skip_list: Vec<SkipEntry>,
    /// Optional pointer to the originating `llm_calls.id` row.
    #[serde(default)]
    pub llm_call_id: Option<String>,
}

#[tool_router(router = write_playbook_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "write_playbook",
        description = "Persist a structured pre-market playbook for `date` (`YYYY-MM-DD`, ET) + `account`. The server assigns a monotonic `generation_id` per `(date, account)` — never pass one. Each `ranked_setups` entry MUST carry `symbol`, `bias` (`long`|`short`), `trigger`, `entry`, `invalidation`, `target_1`, `conviction` (A|B|C), `rationale_md`; `target_2` and `evidence_refs` are optional. `skip_list` entries carry `{symbol, reason}`. Either list may be empty. Returns `{ date, account, generation_id, n_setups, n_skip, generated_at }`."
    )]
    pub async fn write_playbook(
        &self,
        Parameters(args): Parameters<WritePlaybookArgs>,
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

        let input_for_audit = json!({
            "date": args.date,
            "account": args.account,
            "n_setups": args.ranked_setups.len(),
            "n_skip": args.skip_list.len(),
        });
        let audit_id = match record_audit(
            &self.db,
            "write_playbook",
            &input_for_audit,
            &self.caller,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let store = PlaybookStore::new(self.db.clone());
        let req = WritePlaybookRequest {
            date,
            account: args.account.clone(),
            ranked_setups: args.ranked_setups,
            skip_list: args.skip_list,
            llm_call_id: args.llm_call_id,
        };

        match store.write(req).await {
            Ok(outcome) => {
                let n_setups = outcome.playbook.ranked_setups.len();
                let n_skip = outcome.playbook.skip_list.len();
                stamp_audit_summary(
                    &self.db,
                    audit_id,
                    &format!(
                        "playbooks date={} account={} generation_id={} n_setups={} n_skip={}",
                        outcome.playbook.date,
                        outcome.playbook.account,
                        outcome.playbook.generation_id,
                        n_setups,
                        n_skip,
                    ),
                )
                .await;
                emit_event(
                    &self.emitter,
                    AppEvent::PlaybookWritten {
                        date: outcome.playbook.date,
                        account: outcome.playbook.account.clone(),
                        generation_id: outcome.playbook.generation_id,
                        n_setups,
                        n_skip,
                    },
                )
                .await;
                map_tool_result::<_, String>(Ok(json!({
                    "date": outcome.playbook.date.to_string(),
                    "account": outcome.playbook.account,
                    "generation_id": outcome.playbook.generation_id,
                    "n_setups": n_setups,
                    "n_skip": n_skip,
                    "generated_at": outcome.playbook.generated_at,
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
    use crate::services::playbooks::{
        Conviction, EvidenceRef, PlaybookStore, RankedSetup, SetupBias, SkipEntry,
    };

    fn sample_setup(symbol: &str) -> RankedSetup {
        RankedSetup {
            symbol: symbol.into(),
            bias: SetupBias::Long,
            trigger: "reclaim HOD".into(),
            entry: "100".into(),
            invalidation: "lose 95".into(),
            target_1: "110".into(),
            target_2: Some("120".into()),
            conviction: Conviction::A,
            rationale_md: "good".into(),
            evidence_refs: vec![EvidenceRef {
                source: "news".into(),
                note: "8-K".into(),
            }],
        }
    }

    fn good_args() -> WritePlaybookArgs {
        WritePlaybookArgs {
            date: "2026-05-05".into(),
            account: "U1234567".into(),
            ranked_setups: vec![sample_setup("TSLA"), sample_setup("NVDA")],
            skip_list: vec![SkipEntry {
                symbol: "AAPL".into(),
                reason: "earnings AMC".into(),
            }],
            llm_call_id: Some("llm-9".into()),
        }
    }

    #[tokio::test]
    async fn write_playbook_persists_with_audit_and_event() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .write_playbook(Parameters(good_args()))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["date"], "2026-05-05");
        assert_eq!(body["account"], "U1234567");
        assert_eq!(body["generation_id"], 1);
        assert_eq!(body["n_setups"], 2);
        assert_eq!(body["n_skip"], 1);

        // Stored.
        let store = PlaybookStore::new(handler.db.clone());
        let pb = store
            .read_latest(
                chrono::NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(),
                "U1234567",
            )
            .await
            .unwrap()
            .expect("row");
        assert_eq!(pb.ranked_setups.len(), 2);
        assert_eq!(pb.skip_list.len(), 1);

        // Audit row.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].tool, "write_playbook");
    }

    #[tokio::test]
    async fn write_playbook_assigns_monotonic_generation_id() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        for expected in 1..=3 {
            let r = handler
                .write_playbook(Parameters(good_args()))
                .await
                .expect("ok");
            assert_eq!(r.is_error, Some(false));
            let body = r.structured_content.expect("structured");
            assert_eq!(body["generation_id"], expected);
        }

        let store = PlaybookStore::new(handler.db.clone());
        assert_eq!(store.count().await.unwrap(), 3);

        // Each call writes its own audit row.
        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 3);
    }

    #[tokio::test]
    async fn write_playbook_invalid_date_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let mut args = good_args();
        args.date = "garbage".into();
        let r = handler
            .write_playbook(Parameters(args))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_playbook_empty_account_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let mut args = good_args();
        args.account = "  ".into();
        let r = handler
            .write_playbook(Parameters(args))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn write_playbook_allows_empty_setups_and_skip() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let args = WritePlaybookArgs {
            date: "2026-05-05".into(),
            account: "U1234567".into(),
            ranked_setups: vec![],
            skip_list: vec![],
            llm_call_id: None,
        };
        let r = handler
            .write_playbook(Parameters(args))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["n_setups"], 0);
        assert_eq!(body["n_skip"], 0);
    }
}
