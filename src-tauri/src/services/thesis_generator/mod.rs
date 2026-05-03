//! Phase 17 — LLM-backed thesis generator.
//!
//! `ThesisGenerator` runs after a [`Setup`] is persisted by the tracker
//! runner. It builds an Anthropic Messages-API request with a forced
//! tool call (`emit_thesis`), parses the structured tool input, persists
//! the markdown + full JSON to the row, and re-emits `SetupDetected` so
//! the frontend can swap the thesis-less placeholder for the populated
//! version.
//!
//! Failure handling is intentionally graceful: a transient LLM problem
//! (network, upstream 5xx, budget, missing key) leaves the row's
//! `thesis` column NULL and returns `Ok(None)`. Only programming /
//! storage errors bubble up.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tracing::warn;

use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::types::historical::HistoricalBar;
use crate::ibkr::types::news::NewsItem;
use crate::ibkr::types::tracker::Setup;
use crate::services::llm_service::{
    LlmError, LlmKind, LlmRequest, LlmService, Message, Role, SystemBlock, ToolChoice, ToolSchema,
};
use crate::services::tracker_service::{TrackerError, TrackerService};
use crate::storage::StorageError;

#[cfg(test)]
mod tests;

pub const TOOL_NAME: &str = "emit_thesis";
pub const MODEL: &str = "claude-sonnet-4-6";
const MAX_TOKENS: u32 = 1024;
const MAX_BARS_IN_PROMPT: usize = 20;
const MAX_NEWS_IN_PROMPT: usize = 5;

const SYSTEM_PROMPT: &str = "You are a sober swing trader's analyst. You will receive structured signals — never narrate a chart you cannot see. Cite numeric `raw_signals`. Output ONLY through the `emit_thesis` tool.\n\nStyle:\n- Concise, evidence-first.\n- Name the strategy and explain why the structured signals confirm or weaken it.\n- List concrete invalidation levels (price + reason).\n- Risk-flag anything unusual: low float, recent dilution, earnings-blackout window.";

const TOOL_DESCRIPTION: &str =
    "Emit the structured trade thesis for the detected setup. The markdown body must be 80–250 words and reference numeric raw_signals from the input.";

#[derive(Error, Debug)]
pub enum ThesisError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("tracker: {0}")]
    Tracker(#[from] TrackerError),
    #[error("malformed thesis tool input: {0}")]
    Malformed(String),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidationLevel {
    pub label: String,
    pub price: f64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thesis {
    pub thesis_md: String,
    /// Single-character conviction grade — one of `A`, `B`, `C`.
    pub conviction: char,
    pub invalidation_levels: Vec<InvalidationLevel>,
    pub risk_notes: String,
}

/// Borrowed context handed to [`ThesisGenerator::generate`]. The runner
/// already has these slices on hand from `OwnedMarketContext`, so we
/// avoid round-tripping the bars/news through SQLite a second time.
pub struct ThesisContext<'a> {
    pub daily_bars: &'a [HistoricalBar],
    pub recent_news: &'a [NewsItem],
}

#[derive(Clone)]
pub struct ThesisGenerator {
    llm: Arc<LlmService>,
    tracker: Arc<TrackerService>,
    emitter: Arc<EventEmitter>,
}

impl ThesisGenerator {
    pub fn new(
        llm: Arc<LlmService>,
        tracker: Arc<TrackerService>,
        emitter: Arc<EventEmitter>,
    ) -> Self {
        Self {
            llm,
            tracker,
            emitter,
        }
    }

    /// Build the `LlmRequest` for `setup` + `ctx`. Public so tests can
    /// assert on shape without invoking the network seam.
    pub fn build_request(setup: &Setup, ctx: &ThesisContext<'_>) -> LlmRequest {
        let user_payload = json!({
            "setup": {
                "id": setup.id,
                "symbol": setup.symbol,
                "strategy": setup.strategy,
                "direction": setup.direction,
                "trigger_price": setup.trigger_price,
                "stop_price": setup.stop_price,
                "targets": setup.targets,
                "raw_signals": setup.raw_signals,
                "detected_at": setup.detected_at,
            },
            "bars_summary": summarize_bars(ctx.daily_bars),
            "recent_news": summarize_news(ctx.recent_news),
        });

        LlmRequest {
            kind: LlmKind::Thesis,
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
            setup_id: Some(setup.id),
            loop_name: None,
        }
    }

    /// Parse the tool-input JSON returned by Claude into a typed [`Thesis`].
    pub fn parse_thesis(input: &Value) -> Result<Thesis, ThesisError> {
        let thesis_md = input
            .get("thesis_md")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ThesisError::Malformed("missing thesis_md".into()))?
            .to_string();
        let conviction_s = input
            .get("conviction")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ThesisError::Malformed("missing conviction".into()))?;
        let conviction = parse_conviction(conviction_s).ok_or_else(|| {
            ThesisError::Malformed(format!("conviction '{conviction_s}' not in A|B|C"))
        })?;
        let levels_v = input
            .get("invalidation_levels")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ThesisError::Malformed("missing invalidation_levels".into()))?;
        let mut invalidation_levels = Vec::with_capacity(levels_v.len());
        for entry in levels_v {
            let label = entry
                .get("label")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ThesisError::Malformed("invalidation_levels[].label".into()))?
                .to_string();
            let price = entry
                .get("price")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| ThesisError::Malformed("invalidation_levels[].price".into()))?;
            let reason = entry
                .get("reason")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ThesisError::Malformed("invalidation_levels[].reason".into()))?
                .to_string();
            invalidation_levels.push(InvalidationLevel {
                label,
                price,
                reason,
            });
        }
        let risk_notes = input
            .get("risk_notes")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ThesisError::Malformed("missing risk_notes".into()))?
            .to_string();
        Ok(Thesis {
            thesis_md,
            conviction,
            invalidation_levels,
            risk_notes,
        })
    }

    /// Run the full thesis pipeline for `setup`. Returns:
    /// - `Ok(None)` when the setup already has a thesis (idempotent skip)
    ///   or when the LLM fails for a transient / config reason (we log
    ///   a warning and leave the row untouched).
    /// - `Ok(Some(thesis))` after a successful generation, persistence,
    ///   and event re-emission.
    /// - `Err(_)` for storage / serde / programming errors only.
    pub async fn generate(
        &self,
        setup: &Setup,
        ctx: &ThesisContext<'_>,
    ) -> Result<Option<Thesis>, ThesisError> {
        if setup.thesis.is_some() {
            return Ok(None);
        }

        let request = Self::build_request(setup, ctx);
        let response = match self.llm.message(request).await {
            Ok(r) => r,
            Err(e) => return Ok(handle_llm_error(setup.id, e)),
        };

        let tool_call = match response
            .tool_calls
            .into_iter()
            .find(|c| c.name == TOOL_NAME)
        {
            Some(c) => c,
            None => {
                warn!(
                    setup_id = setup.id,
                    "LLM did not return an `emit_thesis` tool call; skipping persist"
                );
                return Ok(None);
            }
        };

        let thesis = match Self::parse_thesis(&tool_call.input) {
            Ok(t) => t,
            Err(e) => {
                warn!(
                    setup_id = setup.id,
                    "thesis tool input failed to parse: {e}; skipping persist"
                );
                return Ok(None);
            }
        };

        let thesis_json = serde_json::to_value(&thesis)?;
        let updated = self
            .tracker
            .update_setup_thesis(setup.id, thesis.thesis_md.clone(), thesis_json)
            .await?;

        if let Err(e) = self
            .emitter
            .emit(AppEvent::SetupDetected {
                setup: Box::new(updated),
                thesis: Some(thesis.thesis_md.clone()),
            })
            .await
        {
            warn!(setup_id = setup.id, "failed to emit SetupDetected: {e}");
        }

        Ok(Some(thesis))
    }
}

fn handle_llm_error(setup_id: i64, e: LlmError) -> Option<Thesis> {
    match &e {
        LlmError::BudgetExhausted
        | LlmError::Auth
        | LlmError::NoApiKey
        | LlmError::Upstream { .. }
        | LlmError::Network(_)
        | LlmError::Malformed(_)
        | LlmError::UnknownModel(_)
        | LlmError::Backend { .. } => {
            warn!(setup_id, "thesis LLM call failed gracefully: {e}");
            None
        }
        // Storage / serde shouldn't abort generation either — we'd
        // rather leave the row thesis-less than crash the runner.
        LlmError::Storage(_) | LlmError::Serde(_) => {
            warn!(setup_id, "thesis LLM call hit an internal error: {e}");
            None
        }
    }
}

fn parse_conviction(s: &str) -> Option<char> {
    let trimmed = s.trim();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let upper = first.to_ascii_uppercase();
    if matches!(upper, 'A' | 'B' | 'C') {
        Some(upper)
    } else {
        None
    }
}

fn tool_schema() -> ToolSchema {
    ToolSchema {
        name: TOOL_NAME.to_string(),
        description: TOOL_DESCRIPTION.to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "thesis_md": {
                    "type": "string",
                    "description": "Markdown thesis, 80–250 words. Cite numeric raw_signals."
                },
                "conviction": {
                    "type": "string",
                    "enum": ["A", "B", "C"],
                    "description": "A = strongest fit, C = weakest"
                },
                "invalidation_levels": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": { "type": "string" },
                            "price": { "type": "number" },
                            "reason": { "type": "string" }
                        },
                        "required": ["label", "price", "reason"]
                    }
                },
                "risk_notes": {
                    "type": "string",
                    "description": "Any unusual factors (low float, dilution, earnings blackout)."
                }
            },
            "required": ["thesis_md", "conviction", "invalidation_levels", "risk_notes"]
        }),
    }
}

fn summarize_bars(bars: &[HistoricalBar]) -> Vec<Value> {
    let take = MAX_BARS_IN_PROMPT.min(bars.len());
    if take == 0 {
        return Vec::new();
    }
    // Take the most recent `take` bars (assumes ascending order).
    let start = bars.len().saturating_sub(take);
    let mut out = Vec::with_capacity(take);
    let mut prev_close: Option<f64> = if start > 0 {
        Some(bars[start - 1].close)
    } else {
        None
    };
    for bar in &bars[start..] {
        let daily_pct = match prev_close {
            Some(p) if p > 0.0 => Some(((bar.close - p) / p) * 100.0),
            _ => None,
        };
        let entry = json!({
            "time": bar.time,
            "close": bar.close,
            "volume": bar.volume,
            "daily_pct": daily_pct,
        });
        out.push(entry);
        prev_close = Some(bar.close);
    }
    out
}

fn summarize_news(items: &[NewsItem]) -> Vec<Value> {
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
            })
        })
        .collect()
}
