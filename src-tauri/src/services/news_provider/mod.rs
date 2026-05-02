//! `NewsProvider` — the trait that abstracts the news fetch path so call
//! sites (MCP `get_news` tool, the `tracker_get_news` Tauri command, the
//! `TrackerRunner`) don't bind to a specific backend.
//!
//! Phase 7 part A wires the [`alpha_vantage::AlphaVantageNewsProvider`]
//! around the existing [`crate::services::financial_data_service::FinancialDataService::fetch_news_sentiment`]
//! call. Phase 7 part B adds an `IbkrNewsProvider` backed by the released
//! `ibapi = "2.11.x"` news APIs, gated behind the `news_source` settings
//! flag introduced here. Phase 8 deletes the AV news adapter and the
//! flag — only the IBKR provider remains.
//!
//! See [`loop/plan/master.md`](../../../../loop/plan/master.md) "Hard
//! invariants" — particularly #1 (the `NewsItem` shape is the contract),
//! #2 (`Send + Sync + 'static` + dyn-compatible), and #5 (no silent
//! mock-data fallback on the migration path).

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

use crate::ibkr::types::news::NewsItem;

pub mod alpha_vantage;
pub mod ibkr;
pub mod test_support;

#[cfg(test)]
mod tests;

/// Typed errors surfaced by [`NewsProvider::fetch`]. The IBKR side
/// emits `NoSubscription` for TWS error 322; the AV adapter never does.
/// `NotConnected` is reserved for the IBKR provider — the AV adapter
/// surfaces transport failures as `Other` because there's no
/// connection-state distinction on the AV side.
///
/// "No news for symbol" is intentionally NOT an error — both providers
/// return `Ok(Vec::new())` so callers can branch on emptiness without
/// pattern-matching on a variant.
#[derive(Debug, Error)]
pub enum NewsError {
    /// Upstream rate-limited the request. `retry_after` is `None` when
    /// the upstream did not advertise a retry window (AV's free-tier
    /// `Information` payloads do not include one; IBKR's pacing errors
    /// also don't).
    #[error("news upstream rate-limited (retry_after = {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },

    /// IBKR-only: TWS news is reachable but the connected account has
    /// no subscription for the named provider. AV equivalent surfaces
    /// as `Other`. `provider_code` carries the IBKR provider code so
    /// the UI can name the missing subscription explicitly.
    #[error("no news subscription for provider {provider_code}")]
    NoSubscription { provider_code: String },

    /// IBKR-only: TWS not running / not connected. AV equivalent
    /// surfaces as `Other` (no connection state).
    #[error("news upstream not connected")]
    NotConnected,

    /// Payload was retrieved but could not be parsed into [`NewsItem`].
    /// Carries the failing message for log triage.
    #[error("news parse error: {0}")]
    ParseError(String),

    /// Catch-all for anything else (transport, unrecognised AV `Note` /
    /// `Error Message` payloads, IBKR errors that aren't subscription-
    /// or rate-limit-related, missing API key on the AV side). The
    /// message is the upstream `Display` form so log triage stays
    /// cheap.
    #[error("{0}")]
    Other(String),
}

/// Async fetch of recent news for `symbol` over the last
/// `lookback_hours`. Implementations must be `Send + Sync + 'static` so
/// a single `Arc<dyn NewsProvider>` can be `app.manage`'d into Tauri
/// state and shared across the MCP + command surface. Dyn-compatibility
/// is provided by `#[async_trait]`.
///
/// Implementations should treat `symbol` as case-insensitive
/// (uppercased internally) so the `aapl` / `AAPL` distinction never
/// reaches the upstream cache.
///
/// `Ok(Vec::new())` is the canonical "no news for this symbol over the
/// lookback window" signal — callers should NOT treat empty as an
/// error.
#[async_trait]
pub trait NewsProvider: Send + Sync + 'static {
    async fn fetch(&self, symbol: &str, lookback_hours: u32) -> Result<Vec<NewsItem>, NewsError>;
}
