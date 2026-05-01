//! `get_alerts` — read the alerts feed (paged, filterable).
//!
//! Wraps `services::alerts::list_alerts`. The `kind` arg accepts the
//! snake_case alert kind names; an unknown value surfaces as a domain
//! error (`isError: true`) so the agent can recover without the JSON-RPC
//! envelope faulting.

use chrono::{DateTime, Utc};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::ibkr::types::tracker::AlertKind;
use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::alerts::{list_alerts, ListAlertsQuery};

/// Hard cap on `limit` to keep an over-eager agent from pulling the whole
/// feed in one call. Matches the doc on the tool description.
const ALERTS_MAX_LIMIT: u32 = 500;

#[derive(Debug, Deserialize, JsonSchema, Default)]
pub struct GetAlertsArgs {
    /// RFC3339 cutoff. Only alerts fired at or after this instant are
    /// returned. Omit for the full feed (subject to `limit` / `offset`).
    #[serde(default)]
    pub since: Option<String>,
    /// One of `"detected"`, `"invalidated"`, `"target_hit"`,
    /// `"thesis_changed"`. Omit to include all kinds.
    #[serde(default)]
    pub kind: Option<String>,
    /// Page size. Defaults to 50, capped at 500. Values above the cap are
    /// clamped (no error).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Number of rows to skip. Use with `limit` for pagination.
    #[serde(default)]
    pub offset: Option<u32>,
    /// When `true`, return only rows whose `seen` flag is still 0.
    #[serde(default)]
    pub only_unseen: Option<bool>,
}

#[tool_router(router = alerts_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_alerts",
        description = "List the tracker alert feed, newest-first, with optional filters: `since` (RFC3339 cutoff), `kind` (\"detected\" | \"invalidated\" | \"target_hit\" | \"thesis_changed\"), `limit` (default 50, capped at 500), `offset`, `only_unseen`. Use this to surface fresh detector events the user has not yet acknowledged, or to scope research to a specific lifecycle event."
    )]
    pub async fn get_alerts(
        &self,
        Parameters(args): Parameters<GetAlertsArgs>,
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
        let kind = match args.kind.as_deref() {
            None => None,
            Some(s) => match AlertKind::parse(s) {
                Some(k) => Some(k),
                None => {
                    return map_tool_result::<(), String>(Err(format!("unknown alert kind: {s}")));
                }
            },
        };
        let limit = args
            .limit
            .map(|l| l.min(ALERTS_MAX_LIMIT))
            .unwrap_or(50)
            .max(1);
        let offset = args.offset.unwrap_or(0);
        let only_unseen = args.only_unseen.unwrap_or(false);

        let query = ListAlertsQuery {
            limit,
            offset,
            since,
            kind,
            only_unseen,
        };
        let result = list_alerts(&self.db, query)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::types::tracker::{AlertKind, TrackerSource};
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use crate::services::alerts::record_alert;
    use crate::strategies::{Direction, SetupCandidate, TargetLevel};
    use serde_json::json;

    fn candidate() -> SetupCandidate {
        SetupCandidate {
            strategy: "breakout",
            tag: crate::ibkr::types::tracker::StrategyTag::Breakout,
            direction: Direction::Long,
            conviction_signal: 0.0,
            detected_at: chrono::Utc::now(),
            trigger_price: 100.0,
            stop_price: 95.0,
            targets: vec![TargetLevel {
                price: 110.0,
                label: "T1".to_string(),
            }],
            raw_signals: json!({}),
            timeframe: crate::ibkr::types::BarSize::Day1,
        }
    }

    /// Seed two alerts of different kinds and verify both filtering and
    /// the default ordering (newest first).
    #[tokio::test]
    async fn get_alerts_returns_filtered_feed() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        handler
            .tracker
            .add("AAPL", TrackerSource::Manual, None, vec![], None)
            .await
            .unwrap();
        let setup = handler
            .tracker
            .insert_setup("AAPL", &candidate())
            .await
            .unwrap();

        record_alert(&handler.db, setup.id, AlertKind::Detected, json!({"a": 1}))
            .await
            .unwrap();
        // Wait one second so the dedup window doesn't swallow the second insert.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        record_alert(
            &handler.db,
            setup.id,
            AlertKind::TargetHit,
            json!({"target": "T1"}),
        )
        .await
        .unwrap();

        // No filter → both rows.
        let result = handler
            .get_alerts(Parameters(GetAlertsArgs::default()))
            .await
            .expect("ok");
        let arr = result
            .structured_content
            .as_ref()
            .expect("structured_content")
            .as_array()
            .expect("array");
        assert_eq!(arr.len(), 2);
        // Newest-first: target_hit (most recent) before detected.
        assert_eq!(arr[0]["kind"].as_str().unwrap(), "target_hit");

        // Filtered by kind → just the matching row.
        let result = handler
            .get_alerts(Parameters(GetAlertsArgs {
                kind: Some("detected".to_string()),
                ..Default::default()
            }))
            .await
            .expect("ok");
        let arr = result
            .structured_content
            .as_ref()
            .expect("structured_content")
            .as_array()
            .expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["kind"].as_str().unwrap(), "detected");
    }

    #[tokio::test]
    async fn get_alerts_unknown_kind_returns_domain_error() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);

        let result = handler
            .get_alerts(Parameters(GetAlertsArgs {
                kind: Some("nope".to_string()),
                ..Default::default()
            }))
            .await
            .expect("rmcp Ok");
        assert_eq!(result.is_error, Some(true));
        let txt = result
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text");
        assert!(txt.text.contains("nope"), "got: {}", txt.text);
    }
}
