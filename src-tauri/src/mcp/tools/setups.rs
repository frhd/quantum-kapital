//! `get_setups` — read persisted detector setups, optionally filtered by
//! symbol and `since`.
//!
//! Wraps `TrackerService::list_setups`. The plan originally listed a
//! `status` arg; the underlying service does not take one, so this tool
//! intentionally drops it. Agents can post-filter on the `status` field
//! of each returned row.

use chrono::{DateTime, Utc};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;

#[derive(Debug, Deserialize, JsonSchema, Default)]
pub struct GetSetupsArgs {
    /// Restrict to setups for this ticker (case-insensitive).
    #[serde(default)]
    pub symbol: Option<String>,
    /// RFC3339 timestamp; only setups detected at or after this instant
    /// are returned. Omit for the full history.
    #[serde(default)]
    pub since: Option<String>,
}

#[tool_router(router = setups_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_setups",
        description = "Returns persisted strategy setups (any status) for the given symbol since the given timestamp. Newest-first. Use this to recall what the detector pipeline has flagged — including invalidated and completed rows — before forming a thesis. Post-filter on the `status` field for active-only views. Returns `{ items: [Setup, ...], count: N }`."
    )]
    pub async fn get_setups(
        &self,
        Parameters(args): Parameters<GetSetupsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let since: Option<DateTime<Utc>> = match args.since.as_deref() {
            None => None,
            Some(s) => match DateTime::parse_from_rfc3339(s) {
                Ok(dt) => Some(dt.with_timezone(&Utc)),
                Err(e) => {
                    return map_tool_result::<(), String>(Err(format!(
                        "since must be RFC3339: {e}"
                    )));
                }
            },
        };
        let result = self
            .tracker
            .list_setups(args.symbol.as_deref(), since)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::tracker::StrategyTag;
    use crate::ibkr::types::BarSize;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use crate::strategies::{Direction, SetupCandidate, TargetLevel};
    use chrono::Utc;
    use serde_json::json;

    fn candidate(detected_at: DateTime<Utc>) -> SetupCandidate {
        SetupCandidate {
            strategy: "breakout",
            tag: StrategyTag::Breakout,
            direction: Direction::Long,
            conviction_signal: 0.0,
            detected_at,
            trigger_price: 100.0,
            stop_price: 95.0,
            targets: vec![TargetLevel {
                price: 110.0,
                label: "T1".to_string(),
            }],
            raw_signals: json!({}),
            timeframe: BarSize::Day1,
        }
    }

    /// We must seed `tracked_tickers` first because `setups.symbol` carries
    /// a foreign-key constraint to it. Then insert two setups for AAPL,
    /// one for TSLA, and verify the symbol filter narrows correctly.
    #[tokio::test]
    async fn get_setups_filters_by_symbol() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        for sym in ["AAPL", "TSLA"] {
            handler
                .tracker
                .add(
                    sym,
                    crate::ibkr::types::tracker::TrackerSource::Manual,
                    None,
                    vec![],
                    None,
                )
                .await
                .unwrap();
        }

        handler
            .tracker
            .insert_setup("AAPL", &candidate(Utc::now()))
            .await
            .unwrap();
        handler
            .tracker
            .insert_setup("AAPL", &candidate(Utc::now()))
            .await
            .unwrap();
        handler
            .tracker
            .insert_setup("TSLA", &candidate(Utc::now()))
            .await
            .unwrap();

        let result = handler
            .get_setups(Parameters(GetSetupsArgs {
                symbol: Some("AAPL".to_string()),
                since: None,
            }))
            .await
            .expect("tool ok");
        let body = result
            .structured_content
            .as_ref()
            .expect("structured_content");
        assert_eq!(body["count"].as_u64().unwrap(), 2);
        let arr = body["items"].as_array().expect("items array");
        assert_eq!(arr.len(), 2);
        for row in arr {
            assert_eq!(row["symbol"].as_str().unwrap(), "AAPL");
        }
    }

    #[tokio::test]
    async fn get_setups_invalid_since_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let result = handler
            .get_setups(Parameters(GetSetupsArgs {
                symbol: None,
                since: Some("not-a-timestamp".to_string()),
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("RFC3339"), "got: {}", txt.text);
    }
}
