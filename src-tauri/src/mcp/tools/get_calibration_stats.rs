//! Phase 8 — `get_calibration_stats` MCP read tool.
//!
//! Per-conviction outcome rollup over a windowed timeframe so the
//! agent (and the eval dashboard) can answer "are A-conviction calls
//! actually winning?". Window is in calendar days; the cutoff is
//! `now - window_days * 86400` (UTC seconds).

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
pub struct GetCalibrationStatsArgs {
    /// Window in calendar days. Defaults to 30, clamped to [1, 365].
    #[serde(default)]
    pub window_days: Option<i64>,
}

#[tool_router(router = get_calibration_stats_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_calibration_stats",
        description = "Per-conviction outcome rollup over the last `window_days` (default 30). Returns `{ window_days, since_unix, buckets: [{ conviction, total, hit_target, hit_entry, hit_invalidation, drifted, no_movement, skipped, unparseable, win_rate, target_rate }], overall }`. `win_rate` = (hit_target + hit_entry) / scoreable; `target_rate` = hit_target / scoreable. Scoreable excludes `skipped` and `unparseable`."
    )]
    pub async fn get_calibration_stats(
        &self,
        Parameters(args): Parameters<GetCalibrationStatsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let window_days = args
            .window_days
            .unwrap_or(DEFAULT_WINDOW_DAYS)
            .clamp(1, MAX_WINDOW_DAYS);
        let since_unix = Utc::now().timestamp() - window_days * 86_400;
        match eval_harness::calibration_stats(&self.db, window_days, since_unix).await {
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
    async fn get_calibration_stats_returns_envelope_for_empty_db() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_calibration_stats(Parameters(GetCalibrationStatsArgs { window_days: None }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["window_days"].as_i64().unwrap(), 30);
        assert!(body["buckets"].is_array());
        assert_eq!(body["overall"]["total"].as_i64().unwrap(), 0);
    }

    #[tokio::test]
    async fn get_calibration_stats_clamps_window() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        let r = handler
            .get_calibration_stats(Parameters(GetCalibrationStatsArgs {
                window_days: Some(10_000),
            }))
            .await
            .expect("ok");
        let body = r.structured_content.expect("structured");
        assert_eq!(body["window_days"].as_i64().unwrap(), 365);
    }
}
