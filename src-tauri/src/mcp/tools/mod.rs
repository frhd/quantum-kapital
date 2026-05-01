//! MCP tool plumbing shared across the read-only tool surface.
//!
//! - One file per tool (`budget.rs`, `watchlist.rs`, `setups.rs`,
//!   `alerts.rs`, `news.rs`, `bars.rs`, `fundamentals.rs`). Each file
//!   owns a `#[tool_router]` block composed into `McpHandler` via
//!   `ToolRouter::Add`.
//! - `test_support.rs` lifts the `Db` + `FixedClock` + handler-builder
//!   helpers out of the per-tool tests so each new tool re-uses them.
//! - `reads.rs` and `types.rs` remain as legacy stubs for now; future
//!   shared types land in `types.rs`.
//! - This file holds cross-tool adapter helpers (e.g. service-error → MCP
//!   `CallToolResult` mapping) so each tool stays a thin wrapper.

#![allow(dead_code)] // helpers consumed by tools added in Steps 5–7.

pub mod account_summary;
pub mod alerts;
pub mod bars;
pub mod budget;
pub mod fundamentals;
pub mod news;
pub mod positions;
pub mod quote;
pub mod reads;
pub mod scanner;
pub mod setups;
pub mod test_support;
pub mod types;
pub mod watchlist;

use std::fmt::Display;

use rmcp::{
    model::{CallToolResult, Content},
    serde_json, ErrorData as McpError,
};
use serde::Serialize;

/// Adapt a service-level `Result<T, E>` to rmcp's `Result<CallToolResult, McpError>`.
///
/// Service-error semantics (matching the Anthropic MCP convention): a domain
/// error (`Err(E)`) becomes a successful JSON-RPC reply containing
/// `{ isError: true, content: [...] }` so the LLM can read the message and
/// recover, rather than an exceptional JSON-RPC `error` envelope which would
/// bubble up as a transport failure on the client side. Only true protocol
/// faults (serialization bugs etc.) bubble up as `McpError`.
pub fn map_tool_result<T, E>(result: Result<T, E>) -> Result<CallToolResult, McpError>
where
    T: Serialize,
    E: Display,
{
    match result {
        Ok(value) => {
            let raw = serde_json::to_value(&value).map_err(|e| {
                McpError::internal_error(format!("serialize tool result: {e}"), None)
            })?;
            // MCP convention: `structuredContent` MUST be a JSON object,
            // never a top-level array (a tool that publishes a structured-
            // output schema must publish a JSON-Schema object schema).
            // Wrap arrays into `{ items: [...], count: N }` so every list-
            // returning tool gets a uniform envelope without per-tool
            // boilerplate. Object payloads pass through unchanged.
            let json = match raw {
                serde_json::Value::Array(arr) => {
                    let count = arr.len();
                    serde_json::json!({ "items": arr, "count": count })
                }
                other => other,
            };
            Ok(CallToolResult::structured(json))
        }
        Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Sample {
        ticker: String,
        score: u32,
    }

    #[test]
    fn map_tool_result_ok_returns_structured_success() {
        let r: Result<Sample, &str> = Ok(Sample {
            ticker: "TSLA".into(),
            score: 7,
        });
        let out = map_tool_result(r).expect("ok mapping");
        assert_eq!(out.is_error, Some(false));
        let body = out.structured_content.expect("structured present");
        assert_eq!(body["ticker"], "TSLA");
        assert_eq!(body["score"], 7);
    }

    /// Locks in the `{ items, count }` envelope contract for top-level
    /// JSON arrays. MCP clients (e.g. Claude Code) reject
    /// `structuredContent` that is a top-level array — every list-
    /// returning tool relies on this auto-wrap to stay protocol-compliant.
    #[test]
    fn map_tool_result_wraps_top_level_array_into_items_count() {
        let r: Result<Vec<i32>, &str> = Ok(vec![1, 2, 3]);
        let out = map_tool_result(r).expect("ok mapping");
        assert_eq!(out.is_error, Some(false));
        let body = out.structured_content.expect("structured present");
        assert!(body.is_object(), "envelope must be a JSON object");
        let items = body["items"].as_array().expect("items array");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_i64().unwrap(), 1);
        assert_eq!(items[1].as_i64().unwrap(), 2);
        assert_eq!(items[2].as_i64().unwrap(), 3);
        assert_eq!(body["count"].as_u64().unwrap(), 3);
    }

    #[test]
    fn map_tool_result_err_returns_is_error_true_with_text_content() {
        let r: Result<Sample, &str> = Err("symbol not in watchlist");
        let out = map_tool_result(r).expect("err is still Ok at the rmcp layer");
        assert_eq!(out.is_error, Some(true));
        assert!(out.structured_content.is_none());
        let txt = out
            .content
            .first()
            .and_then(|c| c.as_text())
            .expect("text content present");
        assert_eq!(txt.text, "symbol not in watchlist");
    }
}
