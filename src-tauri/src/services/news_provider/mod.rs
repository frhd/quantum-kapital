//! `NewsProvider` — the trait that abstracts the news fetch path so call
//! sites (MCP `get_news` tool, the `tracker_get_news` Tauri command, the
//! `TrackerRunner`) don't bind to a specific backend.
//!
//! Production wires [`ibkr::IbkrNewsProvider`], backed by the released
//! `ibapi = "2.11.x"` news APIs. The Alpha Vantage news adapter was
//! removed when the IBKR backend became the default.
//!
//! Hard invariants: the `NewsItem` shape is the contract (adding a
//! field requires updating every impl in lockstep); impls are
//! `Send + Sync + 'static` and dyn-compatible; no silent mock-data
//! fallback on errors — typed errors propagate.

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

use crate::ibkr::types::news::NewsItem;

pub mod ibkr;
pub mod test_support;

#[cfg(test)]
mod tests;

/// Typed errors surfaced by [`NewsProvider::fetch`]. The IBKR provider
/// emits `NoSubscription` for TWS error 322 and `NotConnected` when TWS
/// is unreachable.
///
/// "No news for symbol" is intentionally NOT an error — providers
/// return `Ok(Vec::new())` so callers can branch on emptiness without
/// pattern-matching on a variant.
#[derive(Debug, Error)]
pub enum NewsError {
    /// Upstream rate-limited the request. `retry_after` is `None` when
    /// the upstream did not advertise a retry window (IBKR's pacing
    /// errors don't).
    #[error("news upstream rate-limited (retry_after = {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },

    /// TWS news is reachable but the connected account has no
    /// subscription for the named provider. `provider_code` carries the
    /// IBKR provider code so the UI can name the missing subscription
    /// explicitly.
    #[error("no news subscription for provider {provider_code}")]
    NoSubscription { provider_code: String },

    /// TWS not running / not connected.
    #[error("news upstream not connected")]
    NotConnected,

    /// Payload was retrieved but could not be parsed into [`NewsItem`].
    /// Carries the failing message for log triage.
    #[error("news parse error: {0}")]
    ParseError(String),

    /// Catch-all for transport, unrecognised payloads, or IBKR errors
    /// that aren't subscription- or rate-limit-related. The message is
    /// the upstream `Display` form so log triage stays cheap.
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
