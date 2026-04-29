//! Phase 18 — LLM-backed decay-watcher.
//!
//! For every persisted [`Setup`] flagged `Active`, the intraday
//! scheduler asks Claude Haiku 4.5 every 5 minutes "given the original
//! thesis and the most recent bars, is this setup still valid?". The
//! response is a structured `emit_decay` tool call carrying:
//!
//!   - `still_valid: bool`
//!   - `outcome: still_valid | invalidated | target_hit | thesis_changed`
//!   - `reason: string`
//!   - optional `suggested_action: string` (informational only — we
//!     never place orders).
//!
//! The trait surface ([`DecayWatcher::check`]) stays narrow so the
//! intraday scheduler can keep iterating active setups without owning
//! any LLM concerns. [`DecayWatcherStub`] (always-`StillValid`) is kept
//! around for tests and as a trivial no-op fallback when the LLM stack
//! is intentionally disabled.
//!
//! Failure handling is intentionally graceful. Every transient /
//! configuration / parse problem (`BudgetExhausted`, `Auth`, `Upstream`,
//! network blips, missing API key, malformed tool input, bars-fetch
//! failure, watcher called too soon after detection) collapses to
//! [`DecayDecision::skipped`] — the scheduler logs and continues
//! without flipping the setup row. That keeps the budget kill-switch
//! from snowballing into spurious invalidations.

#![allow(dead_code)] // some constructors are exercised only by tests / Phase 18+ callers.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tracing::warn;

use crate::ibkr::types::historical::{BarSize, HistoricalBar};
use crate::ibkr::types::tracker::Setup;
use crate::services::historical_data_service::Lookback;
use crate::services::llm_service::{
    LlmKind, LlmRequest, LlmService, Message, Role, SystemBlock, ToolChoice, ToolSchema,
};
use crate::services::tracker_runner::BarsFetcher;

#[cfg(test)]
mod tests;

pub const TOOL_NAME: &str = "emit_decay";
pub const MODEL: &str = "claude-haiku-4-5";
const MAX_TOKENS: u32 = 512;
const RECENT_BARS_LIMIT: usize = 12;
const INTRADAY_BAR_SIZE: BarSize = BarSize::Min15;

/// A setup detected less than this long ago is too noisy to evaluate —
/// the first few intraday bars after detection routinely whipsaw and
/// would cause spurious invalidations.
pub const FRESHNESS_GRACE: ChronoDuration = ChronoDuration::minutes(30);

const SYSTEM_PROMPT: &str = "You watch a single trade setup. Given the original thesis and the most recent bars, decide if it is still valid. Output ONLY through the `emit_decay` tool. Be terse.\n\nDecide:\n- `still_valid` true with `outcome: still_valid` → leave the setup alone.\n- `still_valid` false with `outcome: invalidated` when the structure breaks (stop hit, level reclaimed against direction).\n- `still_valid` false with `outcome: target_hit` when a take-profit level on the setup is reached.\n- `still_valid` false with `outcome: thesis_changed` when the underlying premise no longer holds even though no level was breached (e.g. character of tape changed).\n\n`reason` is required and must cite a numeric level or bar.";

const TOOL_DESCRIPTION: &str =
    "Emit a decay verdict for the active setup. Cite the price level or bar that motivates the verdict.";

// ---------------- decision shape ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecayOutcome {
    StillValid,
    Invalidated,
    TargetHit,
    ThesisChanged,
    /// Local-only marker for "we did not consult the LLM this tick"
    /// (too-fresh setup, budget exhausted, transport failure). Never
    /// emitted by the model.
    Skipped,
}

impl DecayOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            DecayOutcome::StillValid => "still_valid",
            DecayOutcome::Invalidated => "invalidated",
            DecayOutcome::TargetHit => "target_hit",
            DecayOutcome::ThesisChanged => "thesis_changed",
            DecayOutcome::Skipped => "skipped",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "still_valid" => Some(DecayOutcome::StillValid),
            "invalidated" => Some(DecayOutcome::Invalidated),
            "target_hit" => Some(DecayOutcome::TargetHit),
            "thesis_changed" => Some(DecayOutcome::ThesisChanged),
            "skipped" => Some(DecayOutcome::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecayDecision {
    pub still_valid: bool,
    pub outcome: DecayOutcome,
    pub reason: Option<String>,
    pub suggested_action: Option<String>,
}

impl DecayDecision {
    pub fn still_valid() -> Self {
        Self {
            still_valid: true,
            outcome: DecayOutcome::StillValid,
            reason: None,
            suggested_action: None,
        }
    }

    pub fn invalidate(reason: impl Into<String>) -> Self {
        Self {
            still_valid: false,
            outcome: DecayOutcome::Invalidated,
            reason: Some(reason.into()),
            suggested_action: None,
        }
    }

    pub fn target_hit(reason: impl Into<String>) -> Self {
        Self {
            still_valid: false,
            outcome: DecayOutcome::TargetHit,
            reason: Some(reason.into()),
            suggested_action: None,
        }
    }

    pub fn thesis_changed(reason: impl Into<String>) -> Self {
        Self {
            still_valid: false,
            outcome: DecayOutcome::ThesisChanged,
            reason: Some(reason.into()),
            suggested_action: None,
        }
    }

    pub fn skipped() -> Self {
        Self {
            still_valid: true,
            outcome: DecayOutcome::Skipped,
            reason: None,
            suggested_action: None,
        }
    }
}

// ---------------- trait + stub ----------------

#[async_trait]
pub trait DecayWatcher: Send + Sync {
    async fn check(&self, setup: &Setup) -> DecayDecision;
}

#[derive(Debug, Default, Clone)]
pub struct DecayWatcherStub;

#[async_trait]
impl DecayWatcher for DecayWatcherStub {
    async fn check(&self, _setup: &Setup) -> DecayDecision {
        DecayDecision::still_valid()
    }
}

// ---------------- LLM-backed implementation ----------------

#[derive(Error, Debug)]
pub enum DecayError {
    #[error("malformed decay tool input: {0}")]
    Malformed(String),
}

/// Clock seam — production wires [`SystemDecayClock`]; tests pin a
/// fixed instant so the freshness grace check is deterministic.
pub trait DecayClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemDecayClock;
impl DecayClock for SystemDecayClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Borrowed snapshot of the data Claude needs to evaluate decay.
/// `current_quote` is just the close of the most recent bar — we don't
/// have a separate live-quote feed wired in yet.
pub struct DecayContext<'a> {
    pub recent_bars: &'a [HistoricalBar],
    pub current_quote: Option<f64>,
}

#[derive(Clone)]
pub struct LlmDecayWatcher {
    llm: Arc<LlmService>,
    bars: Arc<dyn BarsFetcher>,
    clock: Arc<dyn DecayClock>,
}

impl LlmDecayWatcher {
    pub fn new(llm: Arc<LlmService>, bars: Arc<dyn BarsFetcher>) -> Self {
        Self {
            llm,
            bars,
            clock: Arc::new(SystemDecayClock),
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn DecayClock>) -> Self {
        self.clock = clock;
        self
    }

    /// Build the [`LlmRequest`] for a `setup` + freshly fetched intraday
    /// bars. Public so tests can assert request shape without standing
    /// up a real HTTP transport.
    pub fn build_request(setup: &Setup, ctx: &DecayContext<'_>) -> LlmRequest {
        let thesis_md = setup.thesis.clone().unwrap_or_default();
        let invalidation_levels = setup
            .thesis_json
            .as_ref()
            .and_then(|v| v.get("invalidation_levels").cloned())
            .unwrap_or_else(|| Value::Array(vec![]));

        let thesis_block = json!({
            "setup_id": setup.id,
            "symbol": setup.symbol,
            "strategy": setup.strategy,
            "direction": setup.direction,
            "trigger_price": setup.trigger_price,
            "stop_price": setup.stop_price,
            "targets": setup.targets,
            "thesis_md": thesis_md,
            "invalidation_levels": invalidation_levels,
        });

        let user_payload = json!({
            "recent_bars": summarize_bars(ctx.recent_bars),
            "current_quote": ctx.current_quote,
        });

        LlmRequest {
            kind: LlmKind::Decay,
            model: MODEL,
            max_tokens: MAX_TOKENS,
            system: vec![
                SystemBlock {
                    text: SYSTEM_PROMPT.to_string(),
                    cache: true,
                },
                SystemBlock {
                    text: format!(
                        "Active setup context:\n{}",
                        serde_json::to_string(&thesis_block).unwrap_or_else(|_| "{}".to_string())
                    ),
                    cache: true,
                },
            ],
            messages: vec![Message {
                role: Role::User,
                content: serde_json::to_string(&user_payload).unwrap_or_else(|_| "{}".to_string()),
            }],
            tools: Some(vec![tool_schema()]),
            tool_choice: Some(ToolChoice::ForceTool(TOOL_NAME.to_string())),
            setup_id: Some(setup.id),
        }
    }

    /// Parse the `emit_decay` tool input into a typed [`DecayDecision`].
    pub fn parse_decision(input: &Value) -> Result<DecayDecision, DecayError> {
        let still_valid = input
            .get("still_valid")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| DecayError::Malformed("missing still_valid".into()))?;
        let outcome_s = input
            .get("outcome")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecayError::Malformed("missing outcome".into()))?;
        let outcome = DecayOutcome::parse(outcome_s)
            .ok_or_else(|| DecayError::Malformed(format!("unknown outcome '{outcome_s}'")))?;
        if matches!(outcome, DecayOutcome::Skipped) {
            return Err(DecayError::Malformed(
                "outcome 'skipped' is local-only".into(),
            ));
        }
        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let suggested_action = input
            .get("suggested_action")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(DecayDecision {
            still_valid,
            outcome,
            reason,
            suggested_action,
        })
    }
}

#[async_trait]
impl DecayWatcher for LlmDecayWatcher {
    async fn check(&self, setup: &Setup) -> DecayDecision {
        let now = self.clock.now();
        if now.signed_duration_since(setup.detected_at) < FRESHNESS_GRACE {
            return DecayDecision::skipped();
        }

        let raw_bars = match self
            .bars
            .fetch(&setup.symbol, INTRADAY_BAR_SIZE, Lookback::Days(1))
            .await
        {
            Ok(bars) => bars,
            Err(e) => {
                warn!(setup_id = setup.id, "decay bars fetch failed: {e}");
                return DecayDecision::skipped();
            }
        };

        let recent: &[HistoricalBar] = if raw_bars.len() > RECENT_BARS_LIMIT {
            &raw_bars[raw_bars.len() - RECENT_BARS_LIMIT..]
        } else {
            &raw_bars
        };
        let current_quote = recent.last().map(|b| b.close);
        let ctx = DecayContext {
            recent_bars: recent,
            current_quote,
        };

        let request = Self::build_request(setup, &ctx);
        let response = match self.llm.message(request).await {
            Ok(r) => r,
            Err(e) => {
                warn!(setup_id = setup.id, "decay LLM call failed gracefully: {e}");
                return DecayDecision::skipped();
            }
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
                    "decay LLM did not return `emit_decay` tool call"
                );
                return DecayDecision::skipped();
            }
        };

        match Self::parse_decision(&tool_call.input) {
            Ok(d) => d,
            Err(e) => {
                warn!(setup_id = setup.id, "decay tool input failed to parse: {e}");
                DecayDecision::skipped()
            }
        }
    }
}

// ---------------- helpers ----------------

fn tool_schema() -> ToolSchema {
    ToolSchema {
        name: TOOL_NAME.to_string(),
        description: TOOL_DESCRIPTION.to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "still_valid": { "type": "boolean" },
                "outcome": {
                    "type": "string",
                    "enum": ["still_valid", "invalidated", "target_hit", "thesis_changed"]
                },
                "reason": { "type": "string" },
                "suggested_action": { "type": "string" }
            },
            "required": ["still_valid", "outcome", "reason"]
        }),
    }
}

fn summarize_bars(bars: &[HistoricalBar]) -> Vec<Value> {
    bars.iter()
        .map(|b| {
            json!({
                "time": b.time,
                "open": b.open,
                "high": b.high,
                "low": b.low,
                "close": b.close,
                "volume": b.volume,
            })
        })
        .collect()
}
