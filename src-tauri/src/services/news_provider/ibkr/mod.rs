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

use super::{NewsError, NewsProvider};
use client::{IbkrNewsClient, IbkrNewsProviderInfo};

/// Default headline batch size — mirrors AV's `limit=50` so a flag-flip
/// from `alpha_vantage` to `ibkr` does not silently change the volume
/// of headlines feeding the `NewsInterpreter`.
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
}

impl IbkrNewsProvider {
    pub fn new(client: Arc<dyn IbkrNewsClient>, rate_limiter: Arc<IbkrNewsRateLimiter>) -> Self {
        Self {
            client,
            rate_limiter,
            providers: Arc::new(RwLock::new(None)),
            total_results: DEFAULT_TOTAL_RESULTS,
        }
    }

    /// Override the batch size. Defaults to [`DEFAULT_TOTAL_RESULTS`]
    /// (50). Tests use this to keep fixture replay cheap.
    #[allow(dead_code)]
    pub fn with_total_results(mut self, total: u8) -> Self {
        self.total_results = total;
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
        Ok(items)
    }
}
