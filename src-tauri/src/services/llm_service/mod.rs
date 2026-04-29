//! Phase 16 — Anthropic Messages API service with cost ledger + budget kill-switch.
//!
//! - `LlmService::message` is the single entry point. It validates the daily
//!   budget against rows in `llm_calls`, sends through the [`AnthropicHttp`]
//!   trait seam, parses the response, computes cost from [`prices`], and
//!   writes a row to `llm_calls` on success.
//! - The transport seam is intentionally narrow so tests can return canned
//!   JSON without standing up an HTTP server.

#![allow(dead_code)] // Phase 16: surface consumed in Phases 17–20.

pub mod prices;
pub mod types;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use thiserror::Error;
use tracing::warn;

use crate::storage::{Db, StorageError};

pub use types::*;

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// HTTP transport seam. Production wires [`ReqwestAnthropicHttp`]; tests
/// return canned `Value` payloads without a real server.
#[async_trait]
pub trait AnthropicHttp: Send + Sync {
    async fn send_messages(
        &self,
        api_key: &str,
        anthropic_version: &str,
        body: &Value,
    ) -> Result<Value, AnthropicHttpError>;
}

#[derive(Debug, Error)]
pub enum AnthropicHttpError {
    #[error("auth (4xx unauthorized)")]
    Auth,
    #[error("upstream {status}: {body}")]
    Upstream { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
}

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

/// Production transport: POST `{base_url}/v1/messages` with required
/// Anthropic headers + JSON body. `reqwest::Client::json` sets
/// `content-type: application/json` automatically.
pub struct ReqwestAnthropicHttp {
    client: reqwest::Client,
    base_url: String,
}

impl ReqwestAnthropicHttp {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: ANTHROPIC_BASE_URL.to_string(),
        }
    }
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for ReqwestAnthropicHttp {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AnthropicHttp for ReqwestAnthropicHttp {
    async fn send_messages(
        &self,
        api_key: &str,
        anthropic_version: &str,
        body: &Value,
    ) -> Result<Value, AnthropicHttpError> {
        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", anthropic_version)
            .json(body)
            .send()
            .await
            .map_err(|e| AnthropicHttpError::Network(e.to_string()))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AnthropicHttpError::Auth);
        }
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(AnthropicHttpError::Upstream {
                status: status.as_u16(),
                body: body_text,
            });
        }
        resp.json::<Value>()
            .await
            .map_err(|e| AnthropicHttpError::Network(e.to_string()))
    }
}

#[derive(Clone)]
pub struct LlmService {
    http: Arc<dyn AnthropicHttp>,
    db: Arc<Db>,
    clock: Arc<dyn LlmClock>,
    api_key: String,
    daily_budget_usd: f64,
}

impl LlmService {
    pub fn new(api_key: String, db: Arc<Db>, daily_budget_usd: f64) -> Self {
        let http: Arc<dyn AnthropicHttp> = Arc::new(ReqwestAnthropicHttp::new());
        let clock: Arc<dyn LlmClock> = Arc::new(SystemLlmClock);
        Self {
            http,
            db,
            clock,
            api_key,
            daily_budget_usd,
        }
    }
    pub fn with_http(mut self, http: Arc<dyn AnthropicHttp>) -> Self {
        self.http = http;
        self
    }
    pub fn with_clock(mut self, clock: Arc<dyn LlmClock>) -> Self {
        self.clock = clock;
        self
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
        if self.api_key.trim().is_empty() {
            return Err(LlmError::NoApiKey);
        }
        // Budget kill-switch: check today's spend BEFORE any HTTP call.
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

        let body = build_request_body(&req);
        let resp = self
            .http
            .send_messages(&self.api_key, ANTHROPIC_VERSION, &body)
            .await
            .map_err(map_http_err)?;

        let parsed = parse_response(&resp)?;

        // Ledger row.
        let cost = prices::cost_usd(
            req.model,
            parsed.usage.input_tokens,
            parsed.usage.output_tokens,
            parsed.usage.cache_read_input_tokens,
        )
        .ok_or_else(|| LlmError::UnknownModel(req.model.to_string()))?;
        let kind = req.kind.as_str().to_string();
        let model = req.model.to_string();
        let input_tokens = parsed.usage.input_tokens as i64;
        let output_tokens = parsed.usage.output_tokens as i64;
        let cache_read_tokens = parsed.usage.cache_read_input_tokens as i64;
        let setup_id = req.setup_id;
        let called_at = self.clock.now_unix();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO llm_calls (kind, setup_id, model, input_tokens, output_tokens, \
                     cache_read_tokens, cost_usd, called_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        kind,
                        setup_id,
                        model,
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cost,
                        called_at
                    ],
                )?;
                Ok(())
            })
            .await?;

        Ok(parsed)
    }
}

fn map_http_err(e: AnthropicHttpError) -> LlmError {
    match e {
        AnthropicHttpError::Auth => LlmError::Auth,
        AnthropicHttpError::Upstream { status, body } => LlmError::Upstream { status, body },
        AnthropicHttpError::Network(s) => LlmError::Network(s),
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
                        .unwrap()
                        .insert("cache_control".to_string(), json!({ "type": "ephemeral" }));
                }
                block
            })
            .collect();
        body.as_object_mut()
            .unwrap()
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
            .unwrap()
            .insert("tools".to_string(), Value::Array(serialized));
    }

    if let Some(tc) = &req.tool_choice {
        let v = match tc {
            ToolChoice::Auto => json!({ "type": "auto" }),
            ToolChoice::ForceTool(name) => json!({ "type": "tool", "name": name }),
        };
        body.as_object_mut()
            .unwrap()
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
    })
}
