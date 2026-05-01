//! `append_journal_entry` — Phase 7 write tool.
//!
//! Persists a markdown section into `journal_entries` keyed by
//! `(journal_date, section)`. Append-only-by-section semantics
//! (master plan Phase-7 gotcha): a re-run of the EOD review for the
//! same date overwrites the agent's section in place without
//! touching any user-authored sections (which live under different
//! `section` keys).
//!
//! The daily-journal skill is the renderer — it pulls these rows
//! when assembling `journal/YYYY-MM-DD.md`. Keeping the data in
//! SQLite means the MCP server stays free of filesystem path
//! knowledge, and the user's manual notes never collide with the
//! agent's writes.

use chrono::NaiveDate;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::mcp::tools::write_support::{record_audit, stamp_audit_summary};
use crate::services::journal_writer::{self, NewJournalEntry};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AppendJournalEntryArgs {
    /// `YYYY-MM-DD` ET trading-day the section applies to.
    pub date: String,
    /// Section heading the body should land under (e.g.
    /// `"EOD Review (Agent)"`). Trimmed; required.
    pub section: String,
    /// Markdown rendered verbatim under the section heading.
    pub body_md: String,
}

#[tool_router(router = append_journal_entry_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "append_journal_entry",
        description = "Persist a markdown section into the daily trading journal for `date` (`YYYY-MM-DD`, ET). Idempotent on `(date, section)` — re-running with the same key overwrites the body in place without touching other sections (including the user's manual notes). The daily-journal skill renders these rows into `journal/YYYY-MM-DD.md`. Returns `{ entry_id, date, section, written_at }`."
    )]
    pub async fn append_journal_entry(
        &self,
        Parameters(args): Parameters<AppendJournalEntryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let date = match NaiveDate::parse_from_str(&args.date, "%Y-%m-%d") {
            Ok(d) => d,
            Err(e) => {
                return map_tool_result::<(), String>(Err(format!("date must be YYYY-MM-DD: {e}")));
            }
        };
        let section_trimmed = args.section.trim().to_string();
        if section_trimmed.is_empty() {
            return map_tool_result::<(), String>(Err("section must be non-empty".into()));
        }
        if args.body_md.trim().is_empty() {
            return map_tool_result::<(), String>(Err("body_md must be non-empty".into()));
        }

        let input_for_audit = json!({
            "date": args.date,
            "section": section_trimmed,
            "body_len": args.body_md.len(),
        });
        let audit_id = match record_audit(
            &self.db,
            "append_journal_entry",
            &input_for_audit,
            &self.caller,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };

        let outcome = journal_writer::upsert_entry(
            &self.db,
            NewJournalEntry {
                journal_date: date,
                section: section_trimmed,
                body_md: args.body_md,
                written_by: self.caller.clone(),
            },
        )
        .await;

        match outcome {
            Ok(saved) => {
                stamp_audit_summary(
                    &self.db,
                    audit_id,
                    &format!("journal_entries.id={}", saved.id),
                )
                .await;
                map_tool_result::<_, String>(Ok(json!({
                    "entry_id": saved.id,
                    "date": saved.journal_date.to_string(),
                    "section": saved.section,
                    "written_at": saved.written_at.timestamp(),
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
    async fn append_journal_entry_persists_and_audits() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .append_journal_entry(Parameters(AppendJournalEntryArgs {
                date: "2026-05-02".into(),
                section: "EOD Review (Agent)".into(),
                body_md: "## Yesterday's calls\nTSLA hit_entry.".into(),
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        let entry_id = body["entry_id"].as_i64().unwrap();
        assert!(entry_id > 0);
        assert_eq!(body["date"], "2026-05-02");
        assert_eq!(body["section"], "EOD Review (Agent)");

        let entries = journal_writer::list_entries_for_date(
            &handler.db,
            NaiveDate::parse_from_str("2026-05-02", "%Y-%m-%d").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].body_md, "## Yesterday's calls\nTSLA hit_entry.");

        let audits = crate::services::mcp_audit::list(&handler.db, 10, 0)
            .await
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].tool, "append_journal_entry");
        assert_eq!(
            audits[0].result_summary.as_deref(),
            Some(format!("journal_entries.id={entry_id}").as_str())
        );
    }

    #[tokio::test]
    async fn append_journal_entry_overwrites_same_section() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let first = handler
            .append_journal_entry(Parameters(AppendJournalEntryArgs {
                date: "2026-05-02".into(),
                section: "EOD Review (Agent)".into(),
                body_md: "first".into(),
            }))
            .await
            .expect("ok");
        let id1 = first.structured_content.unwrap()["entry_id"]
            .as_i64()
            .unwrap();
        let second = handler
            .append_journal_entry(Parameters(AppendJournalEntryArgs {
                date: "2026-05-02".into(),
                section: "EOD Review (Agent)".into(),
                body_md: "second".into(),
            }))
            .await
            .expect("ok");
        let id2 = second.structured_content.unwrap()["entry_id"]
            .as_i64()
            .unwrap();
        assert_eq!(id1, id2, "must upsert by (date, section)");

        let entries = journal_writer::list_entries_for_date(
            &handler.db,
            NaiveDate::parse_from_str("2026-05-02", "%Y-%m-%d").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].body_md, "second");
    }

    #[tokio::test]
    async fn append_journal_entry_invalid_date_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .append_journal_entry(Parameters(AppendJournalEntryArgs {
                date: "garbage".into(),
                section: "S".into(),
                body_md: "x".into(),
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn append_journal_entry_empty_section_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .append_journal_entry(Parameters(AppendJournalEntryArgs {
                date: "2026-05-02".into(),
                section: "  ".into(),
                body_md: "x".into(),
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn append_journal_entry_empty_body_errors() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .append_journal_entry(Parameters(AppendJournalEntryArgs {
                date: "2026-05-02".into(),
                section: "S".into(),
                body_md: "  \n  ".into(),
            }))
            .await
            .expect("rmcp ok");
        assert_eq!(r.is_error, Some(true));
    }
}
