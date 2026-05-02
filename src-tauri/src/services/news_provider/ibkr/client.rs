//! [`IbkrNewsClient`] — narrow seam over the IBKR news APIs that
//! [`super::IbkrNewsProvider`] depends on. Keeping the seam tight (just
//! these two methods) means the v1 provider can be exercised purely
//! against the Phase 6 fixtures without a live TWS connection, and the
//! live impl in [`crate::ibkr::client::news`] only has to translate
//! `ibapi` types ↔ our domain shape.
//!
//! Mirrors the per-concern client traits already in the codebase
//! ([`crate::services::historical_data_service::HistoricalDataFetcher`],
//! [`crate::services::quote_service::QuoteFetcher`],
//! [`crate::services::auto_scanner::MarketScanner`]). We intentionally
//! do NOT extend [`crate::ibkr::mocks::IbkrClientTrait`] — that broad
//! trait is for end-to-end mocks, while the news flow benefits from a
//! narrower contract that fixture tests can fake without modelling the
//! whole client surface.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::super::NewsError;

/// IBKR news provider directory entry — one row of the response from
/// `client.news_providers()`. Carries the provider `code` (used as the
/// `provider_codes` argument to `historical_news`) and the human-
/// readable `name` (used as the `NewsItem.source` string). Mirrored
/// 1:1 from the Phase 6 `news_providers.json` fixture.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IbkrNewsProviderInfo {
    pub code: String,
    pub name: String,
}

/// A single historical-news row — the parser layer turns this into a
/// [`crate::ibkr::types::news::NewsItem`]. Time is normalised to UTC
/// (`ibapi` returns `time::OffsetDateTime` which the live impl
/// converts to chrono via the unix epoch). `headline` retains the
/// leading `{A:<conids>:L:<locales>}` metadata block that `ibapi`
/// surfaces verbatim — the parser strips it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IbkrHeadline {
    pub time: DateTime<Utc>,
    pub provider_code: String,
    pub article_id: String,
    pub headline: String,
    /// Free-form preview / supplementary text the provider may attach.
    /// Empty for every row in the Phase 6 AAPL fixture; reserved for
    /// providers that do attach a snippet.
    pub extra_data: String,
}

/// Async fetch of the IBKR news APIs the `IbkrNewsProvider` consumes.
/// `Send + Sync + 'static` so a single `Arc<dyn IbkrNewsClient>` can be
/// shared across the provider and the request-driven Tauri commands.
///
/// "No news for this symbol over the lookback window" returns
/// `Ok(Vec::new())` — only TWS-level failures (subscription denial,
/// connection loss, bad payload) surface as [`NewsError`].
#[async_trait]
pub trait IbkrNewsClient: Send + Sync + 'static {
    /// `req_news_providers` → directory of subscribed providers for the
    /// connected account. Cached by the provider on first call.
    async fn news_providers(&self) -> Result<Vec<IbkrNewsProviderInfo>, NewsError>;

    /// `req_historical_news` → up to `total_results` headlines for
    /// `symbol` over the last `lookback_hours`, drawn from any of the
    /// `provider_codes` (which the caller has already filtered to the
    /// subscribed list). The implementation resolves the contract id
    /// internally — symbol resolution is an IBKR concern, not a
    /// provider concern.
    async fn historical_news(
        &self,
        symbol: &str,
        provider_codes: &[String],
        lookback_hours: u32,
        total_results: u8,
    ) -> Result<Vec<IbkrHeadline>, NewsError>;
}

/// Convenience: `Arc<dyn IbkrNewsClient>` is dyn-compatible.
#[allow(dead_code)]
pub type DynIbkrNewsClient = Arc<dyn IbkrNewsClient>;

#[cfg(test)]
mod compile_checks {
    use super::*;

    #[test]
    fn arc_dyn_is_send_sync_static() {
        fn assert_<T: Send + Sync + 'static>() {}
        assert_::<Arc<dyn IbkrNewsClient>>();
    }
}
