//! `SentimentProvider` trait + a tiny `HttpFetcher` HTTP seam.
//!
//! Each provider is a thin wrapper over a single upstream endpoint;
//! the `HttpFetcher` trait exists so per-provider tests can program
//! responses without a real network round-trip. Production wires a
//! shared `reqwest::Client` adapter; tests inject an in-memory map.

use async_trait::async_trait;

use crate::services::social_sentiment::types::SentimentSample;

/// One provider = one upstream sentiment source. The provider is
/// responsible for fetching, parsing, normalising scores to `[-1, 1]`,
/// and producing one [`SentimentSample`] per requested symbol it has
/// data for. Symbols with no signal must be returned as a sample with
/// `is_stale = true` so the orchestrator can persist the gap.
#[async_trait]
pub trait SentimentProvider: Send + Sync {
    /// Stable provider id. Doubles as the value persisted in
    /// `social_sentiment.source`.
    fn id(&self) -> &'static str;

    /// Fetch sentiment for `symbols`. The provider may return fewer
    /// rows than the input (e.g. when an upstream API only surfaces
    /// the top-N) — the orchestrator marks missing symbols stale.
    /// Network/parse errors should bubble as `Err(String)`; the
    /// orchestrator catches them and persists a stale row per symbol
    /// so the agent can tell "we tried" from "we never asked".
    async fn fetch(&self, symbols: &[String]) -> Result<Vec<SentimentSample>, String>;
}

/// Minimal HTTP seam — only what the providers need. Providers store
/// `Arc<dyn HttpFetcher>` and call `get_text` / `get_json`. The trait
/// returns the raw body so each provider owns its own JSON shape and
/// the seam stays free of upstream-specific types.
#[async_trait]
pub trait HttpFetcher: Send + Sync {
    /// GET `url` with optional headers; return the response body as text.
    async fn get_text(&self, url: &str, headers: &[(&str, &str)]) -> Result<String, String>;
}

/// Production HTTP fetcher backed by a shared `reqwest::Client`.
pub struct ReqwestHttpFetcher {
    client: reqwest::Client,
    user_agent: String,
}

impl ReqwestHttpFetcher {
    pub fn new(user_agent: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            user_agent: user_agent.into(),
        }
    }
}

impl Default for ReqwestHttpFetcher {
    fn default() -> Self {
        Self::new("quantum-kapital/0.1 (social-sentiment)")
    }
}

#[async_trait]
impl HttpFetcher for ReqwestHttpFetcher {
    async fn get_text(&self, url: &str, headers: &[(&str, &str)]) -> Result<String, String> {
        let mut req = self
            .client
            .get(url)
            .header(reqwest::header::USER_AGENT, &self.user_agent);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.map_err(|e| format!("http send: {e}"))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("http body: {e}"))?;
        if !status.is_success() {
            return Err(format!("http {status}: {body}"));
        }
        Ok(body)
    }
}

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Arc, Mutex};

/// In-memory mock fetcher. Programs `(url) -> body` and tracks every
/// requested URL so tests can assert what each provider hit.
#[cfg(test)]
#[derive(Clone, Default)]
pub struct MockHttpFetcher {
    inner: Arc<Mutex<MockState>>,
}

#[cfg(test)]
#[derive(Default)]
struct MockState {
    responses: HashMap<String, Result<String, String>>,
    calls: Vec<String>,
}

#[cfg(test)]
#[allow(dead_code)] // helpers consumed across tool/test files asymmetrically
impl MockHttpFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn respond_with(&self, url: &str, body: &str) {
        self.inner
            .lock()
            .unwrap()
            .responses
            .insert(url.to_string(), Ok(body.to_string()));
    }

    pub fn respond_err(&self, url: &str, err: &str) {
        self.inner
            .lock()
            .unwrap()
            .responses
            .insert(url.to_string(), Err(err.to_string()));
    }

    pub fn calls(&self) -> Vec<String> {
        self.inner.lock().unwrap().calls.clone()
    }
}

#[cfg(test)]
#[async_trait]
impl HttpFetcher for MockHttpFetcher {
    async fn get_text(&self, url: &str, _headers: &[(&str, &str)]) -> Result<String, String> {
        let mut guard = self.inner.lock().unwrap();
        guard.calls.push(url.to_string());
        guard
            .responses
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(format!("MockHttpFetcher: no response for {url}")))
    }
}

