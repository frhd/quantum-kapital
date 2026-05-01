//! `get_candidates` — Phase 4 read tool.
//!
//! Returns the agent's candidate-universe inbox: rows surfaced by IBKR
//! scanner profiles + sentiment-surge that haven't been promoted into
//! the watchlist yet. Filters on source substring, score floor, and
//! recency so the agent can ask "what surged in the last 24h?" or
//! "give me the strongest hits regardless of source".

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::candidate_universe::types::CandidateFilter;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCandidatesArgs {
    /// Substring match against any source identifier (case-insensitive)
    /// — e.g. `"sentiment"` matches `sentiment_surge`, `"top_perc_gain"`
    /// matches the IBKR top-gainers scan. `None` returns rows from
    /// every source.
    #[serde(default)]
    pub source: Option<String>,
    /// Lower bound on the merged score (`0.0`–`1.0`). `None` ⇒ no
    /// floor.
    #[serde(default)]
    pub min_score: Option<f64>,
    /// Only rows with `last_seen >= since` (unix seconds). `None` ⇒
    /// any age within the candidate's TTL.
    #[serde(default)]
    pub since_unix: Option<i64>,
    /// `true` includes promoted rows in the response (audit/history
    /// view). Defaults to `false` so the agent's inbox stays clean.
    #[serde(default)]
    pub include_promoted: bool,
    /// Hard cap on returned rows. Defaults to 50; clamped server-side
    /// to a sensible maximum.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[tool_router(router = get_candidates_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_candidates",
        description = "Return the candidate-universe inbox — symbols surfaced by IBKR scanner profiles, sentiment surges, and other staging sources that haven't been promoted into the watchlist yet. Filter with `source` (substring match like \"sentiment\" or \"top_perc_gain\"), `min_score` (0–1), `since_unix` (only rows seen after this timestamp), and `include_promoted` (audit/history view). Returns `{ items, count }` ordered by score DESC."
    )]
    pub async fn get_candidates(
        &self,
        Parameters(args): Parameters<GetCandidatesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let filter = CandidateFilter {
            source_substring: args.source.filter(|s| !s.trim().is_empty()),
            min_score: args.min_score,
            since_last_seen: args.since_unix,
            include_promoted: args.include_promoted,
            limit: args.limit,
        };
        let result = self
            .candidates
            .list(filter)
            .await
            .map_err(|e| e.to_string());
        map_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::test_support::{handler_for_db, make_db};
    use crate::services::candidate_universe::types::{CandidateSource, NewCandidate};
    use serde_json::json;

    async fn seed(handler: &McpHandler, symbol: &str, source: &str, score: f64) {
        handler
            .candidates
            .upsert(NewCandidate {
                symbol: symbol.to_string(),
                source: CandidateSource {
                    source: source.to_string(),
                    score,
                    rank: None,
                    meta: json!({}),
                    last_seen: 0,
                },
                reason_md: Some(format!("seeded for {symbol}")),
                ttl_seconds: 7 * 86_400,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn get_candidates_returns_score_desc_with_count_envelope() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        seed(&handler, "AAA", "scanner_top_perc_gain", 0.5).await;
        seed(&handler, "BBB", "sentiment_surge", 0.9).await;
        seed(&handler, "CCC", "scanner_top_perc_gain", 0.7).await;

        let r = handler
            .get_candidates(Parameters(GetCandidatesArgs {
                source: None,
                min_score: None,
                since_unix: None,
                include_promoted: false,
                limit: None,
            }))
            .await
            .expect("ok");
        assert_eq!(r.is_error, Some(false));
        let body = r.structured_content.expect("structured");
        assert_eq!(body["count"].as_u64().unwrap(), 3);
        let items = body["items"].as_array().unwrap();
        let symbols: Vec<&str> = items
            .iter()
            .map(|i| i["symbol"].as_str().unwrap())
            .collect();
        assert_eq!(symbols, vec!["BBB", "CCC", "AAA"]);
    }

    #[tokio::test]
    async fn get_candidates_filters_by_source_substring() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        seed(&handler, "AAA", "scanner_top_perc_gain", 0.5).await;
        seed(&handler, "BBB", "sentiment_surge", 0.9).await;

        let r = handler
            .get_candidates(Parameters(GetCandidatesArgs {
                source: Some("sentiment".into()),
                min_score: None,
                since_unix: None,
                include_promoted: false,
                limit: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.unwrap();
        assert_eq!(body["count"].as_u64().unwrap(), 1);
        assert_eq!(body["items"][0]["symbol"], "BBB");
    }

    #[tokio::test]
    async fn get_candidates_excludes_promoted_by_default() {
        let (_tmp, db) = make_db();
        let handler = handler_for_db(db);
        seed(&handler, "AAA", "scanner_top_perc_gain", 0.9).await;
        handler.candidates.mark_promoted("AAA").await.unwrap();
        seed(&handler, "BBB", "scanner_top_perc_gain", 0.4).await;

        let r = handler
            .get_candidates(Parameters(GetCandidatesArgs {
                source: None,
                min_score: None,
                since_unix: None,
                include_promoted: false,
                limit: None,
            }))
            .await
            .expect("ok");
        let body = r.structured_content.unwrap();
        let symbols: Vec<&str> = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["symbol"].as_str().unwrap())
            .collect();
        assert_eq!(symbols, vec!["BBB"]);

        let with_promoted = handler
            .get_candidates(Parameters(GetCandidatesArgs {
                source: None,
                min_score: None,
                since_unix: None,
                include_promoted: true,
                limit: None,
            }))
            .await
            .expect("ok");
        let body = with_promoted.structured_content.unwrap();
        assert_eq!(body["count"].as_u64().unwrap(), 2);
    }
}
