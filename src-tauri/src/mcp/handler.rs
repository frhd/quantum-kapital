//! Phase 1 / Step 2 — read-only MCP server handler.
//!
//! Hosts the `rmcp` `ServerHandler` impl plus the `#[tool]`-annotated methods
//! that surface Quantum Kapital's surveillance state to an external MCP
//! client (Claude Code, Inspector, etc.). Tools are read-only by construction
//! — see the surveillance-only rule in the workspace `CLAUDE.md`.

use std::sync::Arc;

use rmcp::{
    handler::server::router::tool::ToolRouter, model::CallToolResult, serde_json::json, tool,
    tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};

use crate::services::llm_service::LlmService;

/// rmcp server handler. One instance per running MCP server.
///
/// Holds only what the currently-registered tools need; future steps grow
/// this to include `Db`, `IbkrClient`, `TrackerService`, etc.
#[derive(Clone)]
pub struct McpHandler {
    llm: Arc<LlmService>,
    tool_router: ToolRouter<Self>,
}

#[tool_router(router = tool_router)]
impl McpHandler {
    pub fn new(llm: Arc<LlmService>) -> Self {
        Self {
            llm,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "get_llm_budget_status",
        description = "Return today's LLM spend versus the configured daily USD budget for the Quantum Kapital app. Use this before kicking off LLM-heavy work to confirm headroom, or to explain why downstream LLM calls are being rejected."
    )]
    pub async fn get_llm_budget_status(&self) -> Result<CallToolResult, McpError> {
        let spent = self
            .llm
            .cost_today_usd()
            .await
            .map_err(|e| McpError::internal_error(format!("cost_today_usd: {e}"), None))?;
        let budget = self.llm.daily_budget_usd();
        let remaining = (budget - spent).max(0.0);
        Ok(CallToolResult::structured(json!({
            "spent_usd": spent,
            "budget_usd": budget,
            "remaining_usd": remaining,
        })))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpHandler {}

/// Test-only constructor that produces an `McpHandler` wired to a fresh
/// SQLite DB at `db_path` seeded with a single today's `llm_calls` row of
/// the requested cost.
///
/// Lives here so the cross-crate integration test in `tests/mcp_tool_call.rs`
/// can construct a realistic handler without exposing `LlmService` / `Db` /
/// the test clock to the public API. Caller is responsible for the temp
/// directory lifetime.
#[doc(hidden)]
pub async fn test_handler_with_seeded_spend(
    db_path: &std::path::Path,
    spent_today_usd: f64,
    daily_budget_usd: f64,
) -> std::io::Result<McpHandler> {
    use std::sync::atomic::AtomicI64;
    use std::sync::Arc;

    use crate::services::llm_service::{LlmClock, LlmService};
    use crate::storage::Db;

    struct FixedClock(AtomicI64);
    impl LlmClock for FixedClock {
        fn now_unix(&self) -> i64 {
            self.0.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    let db =
        Arc::new(Db::open(db_path).map_err(|e| std::io::Error::other(format!("open db: {e}")))?);

    // 2023-11-14 22:13:20 UTC — well after that day's UTC midnight.
    let now: i64 = 1_700_000_000;
    let day_start: i64 = (now / 86_400) * 86_400;

    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
             cache_read_tokens, cost_usd, called_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "thesis",
                "claude-sonnet-4-6",
                0i64,
                0i64,
                0i64,
                spent_today_usd,
                day_start
            ],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("seed llm_calls: {e}")))?;

    let clock: Arc<dyn LlmClock> = Arc::new(FixedClock(AtomicI64::new(now)));
    let llm =
        Arc::new(LlmService::new("test-key".to_string(), db, daily_budget_usd).with_clock(clock));
    Ok(McpHandler::new(llm))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    use tempfile::NamedTempFile;

    use super::McpHandler;
    use crate::services::llm_service::{LlmClock, LlmService};
    use crate::storage::Db;

    fn make_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().expect("tempfile");
        let db = Db::open(tmp.path()).expect("open db");
        (tmp, Arc::new(db))
    }

    struct FixedClock(AtomicI64);
    impl LlmClock for FixedClock {
        fn now_unix(&self) -> i64 {
            self.0.load(Ordering::Relaxed)
        }
    }

    /// Pre-populate `llm_calls` with two rows summing to a known cost
    /// for "today" and assert the tool reports spent / budget / remaining
    /// matching the configured budget.
    #[tokio::test]
    async fn get_llm_budget_status_reports_spent_budget_and_remaining() {
        let (_tmp, db) = make_db();
        // 2023-11-14 22:13:20 UTC — well after today's UTC midnight.
        let now: i64 = 1_700_000_000;
        let day_start: i64 = (now / 86_400) * 86_400;

        // Two rows for today: 0.30 + 0.45 = 0.75 spent.
        db.with_conn(move |conn| {
            for cost in [0.30_f64, 0.45_f64] {
                conn.execute(
                    "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
                     cache_read_tokens, cost_usd, called_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        "thesis",
                        "claude-sonnet-4-6",
                        0i64,
                        0i64,
                        0i64,
                        cost,
                        day_start
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();

        let clock: Arc<dyn LlmClock> = Arc::new(FixedClock(AtomicI64::new(now)));
        let llm = Arc::new(LlmService::new("test-key".to_string(), db, 2.00).with_clock(clock));
        let handler = McpHandler::new(llm);

        let result = handler
            .get_llm_budget_status()
            .await
            .expect("tool returns Ok");

        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content present");
        assert!(
            (body["spent_usd"].as_f64().unwrap() - 0.75).abs() < 1e-9,
            "spent_usd = {}",
            body["spent_usd"]
        );
        assert!(
            (body["budget_usd"].as_f64().unwrap() - 2.00).abs() < 1e-9,
            "budget_usd = {}",
            body["budget_usd"]
        );
        assert!(
            (body["remaining_usd"].as_f64().unwrap() - 1.25).abs() < 1e-9,
            "remaining_usd = {}",
            body["remaining_usd"]
        );
        assert_eq!(result.is_error, Some(false));
    }

    /// When today's spend exceeds the budget the remaining amount must
    /// clamp at zero (the tool is informational; a negative number would
    /// be misleading to the LLM client reading it).
    #[tokio::test]
    async fn get_llm_budget_status_clamps_remaining_at_zero_when_overspent() {
        let (_tmp, db) = make_db();
        let now: i64 = 1_700_000_000;
        let day_start: i64 = (now / 86_400) * 86_400;

        db.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
                 cache_read_tokens, cost_usd, called_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    "thesis",
                    "claude-sonnet-4-6",
                    0i64,
                    0i64,
                    0i64,
                    5.0_f64,
                    day_start
                ],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        let clock: Arc<dyn LlmClock> = Arc::new(FixedClock(AtomicI64::new(now)));
        let llm = Arc::new(LlmService::new("test-key".to_string(), db, 1.00).with_clock(clock));
        let handler = McpHandler::new(llm);

        let body = handler
            .get_llm_budget_status()
            .await
            .unwrap()
            .structured_content
            .expect("structured_content present");

        assert!((body["spent_usd"].as_f64().unwrap() - 5.0).abs() < 1e-9);
        assert!((body["budget_usd"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(body["remaining_usd"].as_f64().unwrap(), 0.0);
    }
}
