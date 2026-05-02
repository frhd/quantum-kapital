//! Phase 8 — `get_prediction_history` MCP read tool.
//!
//! Lets the agent self-introspect on past picks for a single symbol —
//! returns every `predictions` row + joined `outcomes` row (when one
//! exists) since `since_unix`. Newest first.

use chrono::Utc;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::eval_harness;

const DEFAULT_WINDOW_DAYS: i64 = 90;
const MAX_WINDOW_DAYS: i64 = 365;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPredictionHistoryArgs {
    /// Required. Case-insensitive symbol; normalized to upper-case.
    pub symbol: String,
    /// Window in calendar days. Defaults to 90, clamped to [1, 365].
    #[serde(default)]
    pub window_days: Option<i64>,
}

#[tool_router(router = get_prediction_history_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_prediction_history",
        description = "Predictions + joined outcomes for `symbol` over the last `window_days` (default 90). Returns `{ items: [{ prediction: {...}, outcome: null | {...} }], count }` newest-first. `outcome` is null when no scored outcome exists yet."
    )]
    pub async fn get_prediction_history(
        &self,
        Parameters(args): Parameters<GetPredictionHistoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.symbol.trim().is_empty() {
            return map_tool_result::<(), String>(Err("symbol must not be empty".to_string()));
        }
        let window_days = args
            .window_days
            .unwrap_or(DEFAULT_WINDOW_DAYS)
            .clamp(1, MAX_WINDOW_DAYS);
        let since_unix = Utc::now().timestamp() - window_days * 86_400;
        match eval_harness::prediction_history(&self.db, &args.symbol, since_unix).await {
            Ok(rows) => map_tool_result::<_, String>(Ok(rows)),
            Err(e) => map_tool_result::<(), String>(Err(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};

    #[tokio::test]
    async fn get_prediction_history_rejects_empty_symbol() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_prediction_history(Parameters(GetPredictionHistoryArgs {
                symbol: "  ".into(),
                window_days: None,
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn get_prediction_history_returns_empty_when_no_data() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_prediction_history(Parameters(GetPredictionHistoryArgs {
                symbol: "TSLA".into(),
                window_days: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["count"].as_u64().unwrap(), 0);
    }
}
