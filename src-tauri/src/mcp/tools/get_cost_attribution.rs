//! Phase 8 — `get_cost_attribution` MCP read tool.
//!
//! `llm_calls` rolled up by attribution bucket (`loop_name` if set,
//! else `kind:<llm_kind>`) plus cost-per-A-conviction so the dashboard
//! can answer "is the agent expensive vs the value it delivers?".

use chrono::Utc;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::eval_harness;

const DEFAULT_WINDOW_DAYS: i64 = 30;
const MAX_WINDOW_DAYS: i64 = 365;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCostAttributionArgs {
    /// Window in calendar days. Defaults to 30, clamped to [1, 365].
    #[serde(default)]
    pub window_days: Option<i64>,
}

#[tool_router(router = get_cost_attribution_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_cost_attribution",
        description = "LLM cost rollup over the last `window_days` (default 30). Returns `{ window_days, since_unix, total_cost_usd, total_calls, buckets: [{ bucket, call_count, cost_usd }], a_conviction_count, usd_per_a_conviction }`. `bucket` = `loop_name` when set, else `kind:<llm_kind>`. `usd_per_a_conviction` is null when no A-conviction predictions sit in the window."
    )]
    pub async fn get_cost_attribution(
        &self,
        Parameters(args): Parameters<GetCostAttributionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let window_days = args
            .window_days
            .unwrap_or(DEFAULT_WINDOW_DAYS)
            .clamp(1, MAX_WINDOW_DAYS);
        let since_unix = Utc::now().timestamp() - window_days * 86_400;
        match eval_harness::cost_attribution(&self.db, window_days, since_unix).await {
            Ok(stats) => map_tool_result::<_, String>(Ok(stats)),
            Err(e) => map_tool_result::<(), String>(Err(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    #[tokio::test]
    async fn get_cost_attribution_returns_envelope_for_empty_db() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_cost_attribution(Parameters(GetCostAttributionArgs { window_days: None }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["total_cost_usd"].as_f64().unwrap(), 0.0);
        assert_eq!(body["total_calls"].as_i64().unwrap(), 0);
        // a_conviction_count = 0 → usd_per_a_conviction is NaN → JSON null.
        assert!(body["usd_per_a_conviction"].is_null());
    }

    #[tokio::test]
    async fn get_cost_attribution_clamps_window() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_cost_attribution(Parameters(GetCostAttributionArgs {
                window_days: Some(10_000),
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["window_days"].as_i64().unwrap(), 365);
    }
}
