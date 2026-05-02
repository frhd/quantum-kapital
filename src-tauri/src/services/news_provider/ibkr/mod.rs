//! [`IbkrNewsProvider`] — the Phase 7 part-B [`super::NewsProvider`]
//! impl. Backed by the released `ibapi = "2.11.x"` news APIs through
//! the [`IbkrNewsClient`] seam, with the Phase 6 sentiment-loss audit
//! confirming we can leave AV's per-article sentiment fields at their
//! `None` / `Vec::new()` defaults — the per-symbol `NewsInterpreter`
//! verdict path fills the gap.
//!
//! v1 contract:
//! - Caches the `news_providers()` directory on the first `fetch()`
//!   call (warm path is lock-free read).
//! - Each `fetch(symbol)` issues a single `historical_news` call
//!   batching every subscribed `provider_code`. No per-article body
//!   fetch — matches the Phase 7 plan "decisions" entry on body-fetch
//!   policy.
//! - Pacing is enforced by [`IbkrNewsRateLimiter`] (30 calls/min).
//! - Symbol is uppercased before reaching the upstream cache.

pub mod client;
pub mod parsers;
pub mod test_support;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::warn;

use crate::ibkr::types::news::NewsItem;
use crate::middleware::IbkrNewsRateLimiter;
use crate::services::news_cache;
use crate::services::news_interpreter::NewsInterpreter;
use crate::storage::Db;

use super::{NewsError, NewsProvider};
use client::{IbkrNewsClient, IbkrNewsProviderInfo};

/// Default headline batch size — mirrors what AV used to return so the
/// `NewsInterpreter` keeps seeing similar prompt sizes after the AV
/// strip-out.
pub const DEFAULT_TOTAL_RESULTS: u8 = 50;

pub struct IbkrNewsProvider {
    client: Arc<dyn IbkrNewsClient>,
    rate_limiter: Arc<IbkrNewsRateLimiter>,
    /// `news_providers()` cached on first use. The directory rarely
    /// changes within a session — re-fetching on every call would burn
    /// pacing budget for no gain. `RwLock` keeps the warm path
    /// lock-free for reads.
    providers: Arc<RwLock<Option<Vec<IbkrNewsProviderInfo>>>>,
    total_results: u8,
    /// When wired, every successful non-empty `fetch` write-throughs to
    /// `news_cache` so `NewsInterpreter` and the MCP `get_news` tool
    /// can read the same items the runner just saw. The Phase 8
    /// deletion moved this responsibility from the (now-removed) AV
    /// news adapter into the IBKR provider.
    cache_db: Option<Arc<Db>>,
    /// Optional best-effort verdict pass after a fresh write. Mirrors
    /// the AV path — interpreter failures never propagate.
    interpreter: Option<Arc<NewsInterpreter>>,
}

impl IbkrNewsProvider {
    pub fn new(client: Arc<dyn IbkrNewsClient>, rate_limiter: Arc<IbkrNewsRateLimiter>) -> Self {
        Self {
            client,
            rate_limiter,
            providers: Arc::new(RwLock::new(None)),
            total_results: DEFAULT_TOTAL_RESULTS,
            cache_db: None,
            interpreter: None,
        }
    }

    /// Override the batch size. Defaults to [`DEFAULT_TOTAL_RESULTS`]
    /// (50). Tests use this to keep fixture replay cheap.
    #[allow(dead_code)]
    pub fn with_total_results(mut self, total: u8) -> Self {
        self.total_results = total;
        self
    }

    /// Attach a SQLite handle so successful fetches land rows in
    /// `news_cache`. Without this the interpreter and MCP `get_news`
    /// tool see an empty cache forever.
    pub fn with_news_cache(mut self, db: Arc<Db>) -> Self {
        self.cache_db = Some(db);
        self
    }

    /// Attach a [`NewsInterpreter`] for the best-effort verdict pass
    /// that runs after each cache write. Failures are logged and
    /// swallowed.
    pub fn with_news_interpreter(mut self, interpreter: Arc<NewsInterpreter>) -> Self {
        self.interpreter = Some(interpreter);
        self
    }

    async fn provider_codes(&self) -> Result<Vec<IbkrNewsProviderInfo>, NewsError> {
        if let Some(cached) = self.providers.read().await.as_ref() {
            return Ok(cached.clone());
        }
        let mut guard = self.providers.write().await;
        if let Some(cached) = guard.as_ref() {
            // Lost the write race — another caller filled it.
            return Ok(cached.clone());
        }
        let fresh = self.client.news_providers().await?;
        *guard = Some(fresh.clone());
        Ok(fresh)
    }
}

#[async_trait]
impl NewsProvider for IbkrNewsProvider {
    async fn fetch(&self, symbol: &str, lookback_hours: u32) -> Result<Vec<NewsItem>, NewsError> {
        let providers = self.provider_codes().await?;
        if providers.is_empty() {
            // No subscribed providers → IBKR's `historical_news` would
            // reject the request with a missing-provider error, and
            // any items returned would be empty anyway. Surface this
            // as a typed error so the operator sees the misconfiguration
            // (Hard Invariant #5).
            return Err(NewsError::NoSubscription {
                provider_code: "<none subscribed>".to_string(),
            });
        }
        let codes: Vec<String> = providers.iter().map(|p| p.code.clone()).collect();
        let symbol_upper = symbol.trim().to_uppercase();

        self.rate_limiter.acquire().await;
        let headlines = self
            .client
            .historical_news(&symbol_upper, &codes, lookback_hours, self.total_results)
            .await?;

        if headlines.is_empty() {
            // Canonical "no news for symbol" — see trait docs.
            return Ok(Vec::new());
        }

        let items = parsers::headlines_to_news_items(&headlines, &providers);
        if items.is_empty() {
            warn!(
                "IBKR historical_news returned {} headlines for {symbol_upper} but parsing \
                 produced 0 NewsItems — investigate parser regressions",
                headlines.len()
            );
        }

        if !items.is_empty() {
            if let Some(db) = self.cache_db.as_deref() {
                let now = chrono::Utc::now().timestamp();
                if let Err(e) = news_cache::write_cache(db, &symbol_upper, now, &items).await {
                    warn!("news_cache write failed for {symbol_upper} (best-effort): {e}");
                } else if let Some(interpreter) = self.interpreter.as_ref() {
                    if let Err(e) = interpreter.interpret(&symbol_upper).await {
                        warn!(
                            "news interpreter failed for {symbol_upper} (best-effort, continuing): {e}"
                        );
                    }
                }
            }
        }

        Ok(items)
    }
}
