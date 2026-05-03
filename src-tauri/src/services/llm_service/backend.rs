//! Backend transport seam for `LlmService`.
//!
//! `LlmBackend` is the higher-level trait the service calls. Two impls
//! ship today:
//!
//! - [`ApiBackend`] — wraps the existing [`AnthropicHttp`] HTTP path.
//!   Default behavior; selected when `QK_LLM_BACKEND` is unset or
//!   `anthropic`.
//! - [`crate::services::llm_service::cli_backend::ClaudeCliBackend`] —
//!   spawns `claude -p` for subscription-backed inference.
//!
//! The narrow trait surface keeps both backends mockable from
//! `tests.rs` without spawning a real subprocess or HTTP server.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::{build_request_body, parse_response, LlmError, LlmRequest, LlmResponse};

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// HTTP transport seam used by [`ApiBackend`]. Production wires
/// [`ReqwestAnthropicHttp`]; tests return canned `Value` payloads
/// without a real server.
#[async_trait]
pub trait AnthropicHttp: Send + Sync {
    async fn send_messages(
        &self,
        api_key: &str,
        anthropic_version: &str,
        body: &Value,
    ) -> Result<Value, AnthropicHttpError>;
}

#[derive(Debug, thiserror::Error)]
pub enum AnthropicHttpError {
    #[error("auth (4xx unauthorized)")]
    Auth,
    #[error("upstream {status}: {body}")]
    Upstream { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
}

/// Production transport: POST `{base_url}/v1/messages`.
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

/// Higher-level transport seam: takes an `LlmRequest`, returns an
/// `LlmResponse`. `LlmService::message` orchestrates budget checks and
/// the ledger write around this call so neither backend has to know
/// about SQLite.
#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// Synchronous gate run before the budget check. The default Ok is
    /// fine for backends with no startup-time secret to validate. The
    /// API path uses this to fail fast with [`LlmError::NoApiKey`]
    /// when `ANTHROPIC_API_KEY` is empty — preserving the original
    /// pre-refactor error precedence.
    fn precheck(&self) -> Result<(), LlmError> {
        Ok(())
    }

    /// Issue the call. `max_budget_usd` is the per-call ceiling
    /// (≤ daily_budget − cost_today, capped at $1.00). Backends that
    /// honor the cap (e.g. the CLI's `--max-budget-usd`) should pass
    /// it through; the API path ignores it (the kill-switch enforces
    /// the cap on its side).
    async fn call(&self, req: &LlmRequest, max_budget_usd: f64) -> Result<LlmResponse, LlmError>;

    /// Short identifier for the startup INFO log line.
    fn kind(&self) -> &'static str;

    /// Optional version string for the startup INFO log line. The
    /// CLI backend records the output of `claude --version`; the API
    /// backend leaves this `None`.
    fn version(&self) -> Option<&str> {
        None
    }
}

/// API-key-backed Anthropic Messages transport. Existing behavior
/// preserved — cost is left to the service to compute from tokens, so
/// `LlmResponse.cost_usd_override` stays `None` here.
pub struct ApiBackend {
    http: Arc<dyn AnthropicHttp>,
    api_key: String,
}

impl ApiBackend {
    pub fn new(http: Arc<dyn AnthropicHttp>, api_key: String) -> Self {
        Self { http, api_key }
    }

    /// Test-only accessor — `tests.rs` swaps the inner http via
    /// `LlmService::with_http` which rebuilds the backend.
    pub(crate) fn api_key(&self) -> &str {
        &self.api_key
    }
}

#[async_trait]
impl LlmBackend for ApiBackend {
    fn precheck(&self) -> Result<(), LlmError> {
        if self.api_key.trim().is_empty() {
            return Err(LlmError::NoApiKey);
        }
        Ok(())
    }

    async fn call(&self, req: &LlmRequest, _max_budget_usd: f64) -> Result<LlmResponse, LlmError> {
        let body = build_request_body(req);
        let resp = self
            .http
            .send_messages(&self.api_key, ANTHROPIC_VERSION, &body)
            .await
            .map_err(map_http_err)?;
        parse_response(&resp)
    }

    fn kind(&self) -> &'static str {
        "anthropic-api"
    }
}

fn map_http_err(e: AnthropicHttpError) -> LlmError {
    match e {
        AnthropicHttpError::Auth => LlmError::Auth,
        AnthropicHttpError::Upstream { status, body } => LlmError::Upstream { status, body },
        AnthropicHttpError::Network(s) => LlmError::Network(s),
    }
}
