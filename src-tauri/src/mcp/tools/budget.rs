//! `get_llm_budget_status` — today's LLM spend versus the configured budget.
//!
//! Lives in its own file (one file per tool) so `handler.rs` stays under
//! the project's 300-line soft cap as Step 5+ keeps adding tools. The
//! `#[tool_router(router = budget_router)]` annotation gives this block its
//! own tool router; `McpHandler::new` composes it with every other per-tool
//! router via the [`Add`] impl on `ToolRouter`.

use rmcp::{
    handler::server::router::tool::ToolRouter, model::CallToolResult, serde_json::json, tool,
    tool_router, ErrorData as McpError,
};

use crate::mcp::handler::McpHandler;

#[tool_router(router = budget_router, vis = "pub(crate)")]
impl McpHandler {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicI64;
    use std::sync::Arc;

    use crate::mcp::tools::test_support::{handler_with_llm, make_db, FixedClock};
    use crate::services::llm_service::{LlmClock, LlmService};

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
        let db_clone = Arc::clone(&db);
        db_clone
            .with_conn(move |conn| {
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
        let llm = Arc::new(
            LlmService::new("test-key".to_string(), Arc::clone(&db), 2.00).with_clock(clock),
        );
        let handler = handler_with_llm(db, llm);

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

        let db_clone = Arc::clone(&db);
        db_clone
            .with_conn(move |conn| {
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
        let llm = Arc::new(
            LlmService::new("test-key".to_string(), Arc::clone(&db), 1.00).with_clock(clock),
        );
        let handler = handler_with_llm(db, llm);

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
