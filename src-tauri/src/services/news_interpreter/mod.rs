//! Phase 19 — LLM-backed news interpreter.
//!
//! After the [`super::financial_data_service`] writes a fresh news
//! payload to `news_cache`, the interpreter asks Claude Haiku 4.5 to
//! classify the items for one symbol: `tone` (bullish/bearish/neutral),
//! `ep_worthy`, `parabolic_risk`, plus a terse summary. The structured
//! verdict is persisted on the same row in `news_cache.news_verdict_json`
//! and consumed downstream by the EP detector to disambiguate sentiment
//! polarity.
//!
//! Failure handling is intentionally graceful: every transient or
//! configuration LLM error (`BudgetExhausted`, `Auth`, `Upstream`,
//! `Network`, `NoApiKey`, `Malformed`, `UnknownModel`) collapses to
//! `Ok(None)` with a `warn!`. The cache row stays verdict-less and the
//! EP detector falls back to AV's per-ticker sentiment score.

#![allow(dead_code)] // some helpers are exercised only by tests / Phase 19+ callers.

use std::sync::Arc;

use serde_json::{json, Value};
use thiserror::Error;
use tracing::warn;

use crate::ibkr::types::news::{NewsItem, NewsTone, NewsVerdict};
use crate::services::llm_service::{
    LlmError, LlmKind, LlmRequest, LlmService, Message, Role, SystemBlock, ToolChoice, ToolSchema,
};
use crate::services::news_cache::{read_cache_with_verdict, write_verdict, CachedNews};
use crate::storage::{Db, StorageError};

#[cfg(test)]
mod tests;

pub const TOOL_NAME: &str = "emit_news_verdict";
pub const MODEL: &str = "claude-haiku-4-5";
const MAX_TOKENS: u32 = 384;
const MAX_NEWS_IN_PROMPT: usize = 10;

const SYSTEM_PROMPT: &str = "You read 1–10 news items about one stock. Output ONLY through the `emit_news_verdict` tool. Be terse, neutral, evidence-grounded.\n\nDecide:\n- `tone`: `bullish` if the dominant catalyst leans positive, `bearish` if negative, `neutral` if mixed or routine (10-K, dividend declaration, analyst day with no new information).\n- `ep_worthy`: true when the headlines could plausibly drive an episodic-pivot setup (earnings beat/miss, guidance revision, FDA decision, M&A, breakout-grade product news). False for routine filings or stale macro chatter.\n- `parabolic_risk`: true when the items hint at exhaustion / squeeze dynamics (\"short squeeze\", \"meme\", \"halted\", \"vertical\", repeated euphoria) — flag this even when `tone` is bullish.\n- `summary`: one or two sentences citing the most load-bearing headline. Do not editorialize.";

const TOOL_DESCRIPTION: &str =
    "Emit a structured news verdict for the symbol. Cite a headline that motivates the verdict in `summary`.";

#[derive(Error, Debug)]
pub enum NewsError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("malformed news verdict tool input: {0}")]
    Malformed(String),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("cache: {0}")]
    Cache(String),
}

#[derive(Clone)]
pub struct NewsInterpreter {
    llm: Arc<LlmService>,
    db: Arc<Db>,
}

impl NewsInterpreter {
    pub fn new(llm: Arc<LlmService>, db: Arc<Db>) -> Self {
        Self { llm, db }
    }

    /// Build the `LlmRequest` for `symbol` + `items`. Public so tests
    /// can assert request shape (cache control, tool schema, message
    /// payload) without exercising the network seam.
    pub fn build_request(symbol: &str, items: &[NewsItem]) -> LlmRequest {
        let user_payload = json!({
            "symbol": symbol,
            "news": summarize_items(items),
        });

        LlmRequest {
            kind: LlmKind::News,
            model: MODEL,
            max_tokens: MAX_TOKENS,
            system: vec![SystemBlock {
                text: SYSTEM_PROMPT.to_string(),
                cache: true,
            }],
            messages: vec![Message {
                role: Role::User,
                content: serde_json::to_string(&user_payload).unwrap_or_else(|_| "{}".to_string()),
            }],
            tools: Some(vec![tool_schema()]),
            tool_choice: Some(ToolChoice::ForceTool(TOOL_NAME.to_string())),
            setup_id: None,
            loop_name: None,
        }
    }

    /// Parse the `emit_news_verdict` tool input into a typed
    /// [`NewsVerdict`].
    pub fn parse_verdict(input: &Value) -> Result<NewsVerdict, NewsError> {
        let tone_s = input
            .get("tone")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NewsError::Malformed("missing tone".into()))?;
        let tone = NewsTone::parse(tone_s)
            .ok_or_else(|| NewsError::Malformed(format!("unknown tone '{tone_s}'")))?;
        let ep_worthy = input
            .get("ep_worthy")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| NewsError::Malformed("missing ep_worthy".into()))?;
        let parabolic_risk = input
            .get("parabolic_risk")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| NewsError::Malformed("missing parabolic_risk".into()))?;
        let summary = input
            .get("summary")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NewsError::Malformed("missing summary".into()))?
            .to_string();
        Ok(NewsVerdict {
            tone,
            ep_worthy,
            parabolic_risk,
            summary,
        })
    }

    /// Run the news interpreter for `symbol`. Returns:
    /// - `Ok(None)` when there is no cached news row for the symbol,
    ///   when a verdict already exists for the current payload (the
    ///   cache write clears it on every fresh fetch, so a non-NULL
    ///   `news_verdict_json` means "interpreter already ran for this
    ///   payload"), or when the LLM call fails for a transient /
    ///   config reason (logged as a warning).
    /// - `Ok(Some(verdict))` after a successful generation + persist.
    /// - `Err(_)` only for storage / serde / programming errors.
    pub async fn interpret(&self, symbol: &str) -> Result<Option<NewsVerdict>, NewsError> {
        let symbol_upper = symbol.to_uppercase();
        let cached = match read_cache_with_verdict(&self.db, &symbol_upper)
            .await
            .map_err(|e| NewsError::Cache(e.to_string()))?
        {
            Some(c) => c,
            None => return Ok(None),
        };

        // Idempotent skip: a non-NULL verdict_json means the interpreter
        // already ran for the current payload. The cache writer clears
        // this column whenever the payload is replaced.
        if cached.verdict_json.is_some() {
            return Ok(None);
        }

        if cached.items.is_empty() {
            return Ok(None);
        }

        let CachedNews { items, .. } = cached;
        let request = Self::build_request(&symbol_upper, &items);
        let response = match self.llm.message(request).await {
            Ok(r) => r,
            Err(e) => return Ok(handle_llm_error(&symbol_upper, e)),
        };

        let tool_call = match response
            .tool_calls
            .into_iter()
            .find(|c| c.name == TOOL_NAME)
        {
            Some(c) => c,
            None => {
                warn!(
                    symbol = %symbol_upper,
                    "LLM did not return an `emit_news_verdict` tool call; skipping persist"
                );
                return Ok(None);
            }
        };

        let verdict = match Self::parse_verdict(&tool_call.input) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    symbol = %symbol_upper,
                    "news verdict tool input failed to parse: {e}; skipping persist"
                );
                return Ok(None);
            }
        };

        let json = serde_json::to_string(&verdict)?;
        write_verdict(&self.db, &symbol_upper, &json)
            .await
            .map_err(|e| NewsError::Cache(e.to_string()))?;

        Ok(Some(verdict))
    }
}

fn handle_llm_error(symbol: &str, e: LlmError) -> Option<NewsVerdict> {
    match &e {
        LlmError::BudgetExhausted
        | LlmError::Auth
        | LlmError::NoApiKey
        | LlmError::Upstream { .. }
        | LlmError::Network(_)
        | LlmError::Malformed(_)
        | LlmError::UnknownModel(_) => {
            warn!(symbol, "news interpreter LLM call failed gracefully: {e}");
            None
        }
        LlmError::Storage(_) | LlmError::Serde(_) => {
            warn!(
                symbol,
                "news interpreter LLM call hit an internal error: {e}"
            );
            None
        }
    }
}

fn tool_schema() -> ToolSchema {
    ToolSchema {
        name: TOOL_NAME.to_string(),
        description: TOOL_DESCRIPTION.to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "tone": {
                    "type": "string",
                    "enum": ["bullish", "bearish", "neutral"]
                },
                "ep_worthy": { "type": "boolean" },
                "parabolic_risk": { "type": "boolean" },
                "summary": { "type": "string" }
            },
            "required": ["tone", "ep_worthy", "parabolic_risk", "summary"]
        }),
    }
}

fn summarize_items(items: &[NewsItem]) -> Vec<Value> {
    items
        .iter()
        .take(MAX_NEWS_IN_PROMPT)
        .map(|n| {
            json!({
                "title": n.title,
                "summary": n.summary,
                "source": n.source,
                "time_published": n.time_published,
                "overall_sentiment_label": n.overall_sentiment_label,
                "overall_sentiment_score": n.overall_sentiment_score,
            })
        })
        .collect()
}
