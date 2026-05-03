//! Phase 16 — Anthropic Messages API service with cost ledger + budget kill-switch.
//!
//! - `LlmService::message` is the single entry point. It validates the daily
//!   budget against rows in `llm_calls`, dispatches through an
//!   [`LlmBackend`] (API or CLI subprocess), parses the response, computes
//!   cost, and writes a row to `llm_calls` on success.
//! - The transport seam is intentionally narrow so tests can return canned
//!   payloads without standing up an HTTP server or spawning a real
//!   subprocess.

#![allow(dead_code)] // Phase 16: surface consumed in Phases 17–20.

pub mod backend;
pub mod cli_backend;
pub mod prices;
pub mod types;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde_json::{json, Value};
use thiserror::Error;
use tracing::warn;

use crate::storage::{Db, StorageError};

#[allow(unused_imports)]
pub use backend::{
    AnthropicHttp, AnthropicHttpError, ApiBackend, LlmBackend, ReqwestAnthropicHttp,
    ANTHROPIC_BASE_URL, ANTHROPIC_VERSION,
};
#[allow(unused_imports)]
pub use cli_backend::ClaudeCliBackend;
pub use types::*;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("daily budget exhausted")]
    BudgetExhausted,
    #[error("auth: bad ANTHROPIC_API_KEY")]
    Auth,
    #[error("upstream {status}: {body}")]
    Upstream { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("ANTHROPIC_API_KEY is empty")]
    NoApiKey,
    #[error("unknown model: {0}")]
    UnknownModel(String),
    #[error("malformed response: {0}")]
    Malformed(String),
    /// Backend-layer failure (subprocess spawn, envelope parse,
    /// CLI-side `is_error: true`, version probe miss). Catch-all for
    /// failures the API path can't produce. Existing graceful
    /// degraders (e.g. `NewsInterpreter`) treat it like any other
    /// transport error.
    #[error("backend [{stage}]: {message}")]
    Backend { stage: String, message: String },
}

/// Injectable clock — second-resolution to align with `llm_calls.called_at`.
pub trait LlmClock: Send + Sync {
    fn now_unix(&self) -> i64;
}

pub struct SystemLlmClock;
impl LlmClock for SystemLlmClock {
    fn now_unix(&self) -> i64 {
        chrono::Utc::now().timestamp()
    }
}

#[derive(Clone)]
pub struct LlmService {
    backend: Arc<dyn LlmBackend>,
    db: Arc<Db>,
    clock: Arc<dyn LlmClock>,
    daily_budget_usd: f64,
    /// Stashed so the test-only `with_http` builder can rebuild
    /// `ApiBackend` with a swapped HTTP transport without callers
    /// having to thread the api key through. Empty (and unused) when
    /// the service is constructed via `new_with_backend`.
    api_key_for_with_http: String,
}

impl LlmService {
    /// Default constructor — wires the API-backed transport. Kept as
    /// a thin shim over [`Self::new_with_backend`] so call sites that
    /// don't care about backend selection (and existing tests) keep
    /// the same surface.
    pub fn new(api_key: String, db: Arc<Db>, daily_budget_usd: f64) -> Self {
        let http: Arc<dyn AnthropicHttp> = Arc::new(ReqwestAnthropicHttp::new());
        let backend: Arc<dyn LlmBackend> = Arc::new(ApiBackend::new(http, api_key.clone()));
        Self {
            backend,
            db,
            clock: Arc::new(SystemLlmClock),
            daily_budget_usd,
            api_key_for_with_http: api_key,
        }
    }

    /// Backend-aware constructor used by `lib.rs::run` after it
    /// resolves `QK_LLM_BACKEND` and (for the CLI path) runs the
    /// version probe.
    pub fn new_with_backend(
        backend: Arc<dyn LlmBackend>,
        db: Arc<Db>,
        daily_budget_usd: f64,
    ) -> Self {
        Self {
            backend,
            db,
            clock: Arc::new(SystemLlmClock),
            daily_budget_usd,
            api_key_for_with_http: String::new(),
        }
    }

    /// Test helper — replaces the underlying API HTTP transport.
    /// Rebuilds `ApiBackend` with the original api key so existing
    /// tests in `tests.rs` continue to drive the API path through a
    /// fake `AnthropicHttp` without modification.
    pub fn with_http(mut self, http: Arc<dyn AnthropicHttp>) -> Self {
        self.backend = Arc::new(ApiBackend::new(http, self.api_key_for_with_http.clone()));
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn LlmClock>) -> Self {
        self.clock = clock;
        self
    }

    /// Configured daily USD spend cap. Mirror of the private field exposed
    /// for read-only consumers (e.g. the MCP `get_llm_budget_status` tool).
    pub fn daily_budget_usd(&self) -> f64 {
        self.daily_budget_usd
    }

    /// Active backend's identifier (e.g. `"anthropic-api"`,
    /// `"claude-cli"`). Used by the startup INFO log line.
    pub fn backend_kind(&self) -> &'static str {
        self.backend.kind()
    }

    /// Active backend's version string when it has one (CLI path).
    pub fn backend_version(&self) -> Option<String> {
        self.backend.version().map(|s| s.to_string())
    }

    /// Returns the sum of `cost_usd` for rows in `llm_calls` whose `called_at`
    /// is at or after the start of the current UTC day.
    pub async fn cost_today_usd(&self) -> Result<f64, LlmError> {
        let day_start = utc_day_start_unix(self.clock.now_unix());
        let total = self
            .db
            .with_conn(move |conn| {
                let cost: f64 = conn.query_row(
                    "SELECT COALESCE(SUM(cost_usd), 0.0) FROM llm_calls WHERE called_at >= ?1",
                    rusqlite::params![day_start],
                    |row| row.get(0),
                )?;
                Ok(cost)
            })
            .await?;
        Ok(total)
    }

    pub async fn message(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        // Backend-specific pre-call gate (API path: empty key →
        // NoApiKey; CLI path: no-op).
        self.backend.precheck()?;

        // Budget kill-switch: check today's spend BEFORE any backend call.
        let cost_today = self.cost_today_usd().await?;
        if cost_today >= self.daily_budget_usd {
            warn!(
                cost_today,
                budget = self.daily_budget_usd,
                "LLM budget exhausted; rejecting call"
            );
            return Err(LlmError::BudgetExhausted);
        }

        // Validate model up front — pricing table is the source of truth.
        if prices::price_for(req.model).is_none() {
            return Err(LlmError::UnknownModel(req.model.to_string()));
        }

        // Per-call cap: smaller of remaining-budget and $1.00. Floor at
        // a tiny positive value so the CLI doesn't reject a 0 cap when
        // the kill-switch is on the verge of tripping.
        let max_per_call = (self.daily_budget_usd - cost_today).clamp(0.001, 1.0);

        let parsed = self.backend.call(&req, max_per_call).await?;

        // Cost: prefer the backend's authoritative figure (CLI path),
        // fall back to the local pricing table (API path + CLI when
        // the envelope lacks `total_cost_usd`).
        let cost = match parsed.cost_usd_override {
            Some(c) => c,
            None => prices::cost_usd(
                req.model,
                parsed.usage.input_tokens,
                parsed.usage.output_tokens,
                parsed.usage.cache_read_input_tokens,
            )
            .ok_or_else(|| LlmError::UnknownModel(req.model.to_string()))?,
        };
        let kind = req.kind.as_str().to_string();
        let model = req.model.to_string();
        let input_tokens = parsed.usage.input_tokens as i64;
        let output_tokens = parsed.usage.output_tokens as i64;
        let cache_read_tokens = parsed.usage.cache_read_input_tokens as i64;
        let setup_id = req.setup_id;
        let loop_name = req.loop_name.clone();
        let called_at = self.clock.now_unix();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO llm_calls (kind, setup_id, model, input_tokens, output_tokens, \
                     cache_read_tokens, cost_usd, called_at, loop_name) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![
                        kind,
                        setup_id,
                        model,
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cost,
                        called_at,
                        loop_name,
                    ],
                )?;
                Ok(())
            })
            .await?;

        Ok(parsed)
    }
}

/// Floor `now_unix` to the most recent UTC midnight (00:00:00 UTC).
pub fn utc_day_start_unix(now_unix: i64) -> i64 {
    const SECONDS_PER_DAY: i64 = 86_400;
    if now_unix >= 0 {
        (now_unix / SECONDS_PER_DAY) * SECONDS_PER_DAY
    } else {
        // Safe negative-handling, though we don't expect pre-1970 timestamps.
        ((now_unix - SECONDS_PER_DAY + 1) / SECONDS_PER_DAY) * SECONDS_PER_DAY
    }
}

/// Build the JSON body sent to `/v1/messages`. Public for tests.
pub fn build_request_body(req: &LlmRequest) -> Value {
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "messages": req.messages.iter().map(|m| json!({
            "role": m.role.as_str(),
            "content": m.content,
        })).collect::<Vec<_>>(),
    });

    if !req.system.is_empty() {
        let blocks: Vec<Value> = req
            .system
            .iter()
            .map(|b| {
                let mut block = json!({ "type": "text", "text": b.text });
                if b.cache {
                    block
                        .as_object_mut()
                        .expect("json!({...}) always produces a JSON object")
                        .insert("cache_control".to_string(), json!({ "type": "ephemeral" }));
                }
                block
            })
            .collect();
        body.as_object_mut()
            .expect("json!({...}) always produces a JSON object")
            .insert("system".to_string(), Value::Array(blocks));
    }

    if let Some(tools) = &req.tools {
        let serialized: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        body.as_object_mut()
            .expect("json!({...}) always produces a JSON object")
            .insert("tools".to_string(), Value::Array(serialized));
    }

    if let Some(tc) = &req.tool_choice {
        let v = match tc {
            ToolChoice::Auto => json!({ "type": "auto" }),
            ToolChoice::ForceTool(name) => json!({ "type": "tool", "name": name }),
        };
        body.as_object_mut()
            .expect("json!({...}) always produces a JSON object")
            .insert("tool_choice".to_string(), v);
    }

    body
}

/// Parse Anthropic's Messages API response into our typed shape. Public for tests.
pub fn parse_response(value: &Value) -> Result<LlmResponse, LlmError> {
    let content = value
        .get("content")
        .and_then(|v| v.as_array())
        .ok_or_else(|| LlmError::Malformed("missing `content` array".into()))?;

    let mut text: Option<String> = None;
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    for block in content {
        match block.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    text = Some(t.to_string());
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| LlmError::Malformed("tool_use missing name".into()))?
                    .to_string();
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                tool_calls.push(ToolCall { name, input });
            }
            _ => { /* ignore unknown block types */ }
        }
    }

    let usage = value
        .get("usage")
        .map(|u| Usage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            cache_read_input_tokens: u
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_creation_input_tokens: u
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        })
        .unwrap_or_default();

    Ok(LlmResponse {
        text,
        tool_calls,
        usage,
        cost_usd_override: None,
    })
}
