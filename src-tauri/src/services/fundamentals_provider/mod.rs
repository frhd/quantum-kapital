//! `FundamentalsProvider` — the trait that abstracts the fundamentals
//! fetch path so call sites (MCP `get_fundamentals` tool, the analysis
//! Tauri commands) don't bind to a specific backend.
//!
//! Production wires a [`composite::CompositeFundamentalsProvider`] over
//! a [`manual::ManualFundamentalsProvider`] (SQLite-backed, written by
//! the MCP `set_fundamentals` tool) and an
//! [`alpha_vantage::AlphaVantageFundamentalsProvider`] (opportunistic
//! fallback wrapping [`crate::services::financial_data_service::FinancialDataService`]).
//! Manual rows always win; AV is only consulted when the manual store
//! is empty for a symbol.
//!
//! Hard invariants worth remembering: the `FundamentalData` shape is
//! the contract (adding a field requires updating every impl in
//! lockstep), and the tracker pipeline MUST NOT depend on this trait —
//! enforced by `tests/tracker_no_fundamentals.rs`.

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

use crate::ibkr::types::FundamentalData;

pub mod alpha_vantage;
pub mod av_call_ledger;
pub mod composite;
pub mod manual;
pub mod test_support;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "av_call_ledger_tests.rs"]
mod av_call_ledger_tests;

/// Typed errors surfaced by [`FundamentalsProvider::fetch`]. The
/// `DailyBudgetExhausted` / `PerSymbolBudgetExhausted` variants are
/// emitted by Phase 5's [`composite::CompositeFundamentalsProvider`]
/// when its AV-side guards trip; the Phase 3 AV adapter only emits the
/// first five.
///
/// Stringly-typed `ParseError` / `Other` carry the upstream message so
/// the UI can render a meaningful banner without the backend leaking
/// transport details into the `Display` impl.
#[derive(Debug, Error)]
pub enum FundamentalsError {
    /// Upstream rate-limited the request. `retry_after` is `None` when
    /// the upstream did not advertise a retry window (AV's free-tier
    /// `Information` payloads do not include one).
    #[error("fundamentals upstream rate-limited (retry_after = {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },

    /// Upstream is unreachable (TWS not connected, network down, etc.).
    /// Distinct from `RateLimited` so the UI can prompt the user to
    /// reconnect rather than wait it out.
    #[error("fundamentals upstream not connected")]
    NotConnected,

    /// Payload was retrieved but could not be parsed into [`FundamentalData`].
    /// Carries the failing message for log triage.
    #[error("fundamentals parse error: {0}")]
    ParseError(String),

    /// Upstream confirmed the symbol exists but has no fundamentals
    /// (too-new ticker, ADR with sparse coverage, etc.). Distinct from
    /// `Other` so the UI can show a "no data" empty state rather than
    /// a generic failure banner.
    #[error("no fundamentals available for {0}")]
    NotFound(String),

    /// Phase 5: emitted by the composite provider's AV branch when the
    /// daily call ledger is at the hard cap (default `25/25`).
    /// `hit_count` is the count at the time the cap tripped so the UI
    /// can render "you've used 25/25 AV calls today" without a second
    /// query. The Phase 3 AV adapter never emits this variant directly.
    #[error("daily fundamentals budget exhausted ({hit_count} calls today)")]
    DailyBudgetExhausted { hit_count: u32 },

    /// Phase 5: emitted by the composite provider's AV branch when the
    /// per-symbol-per-day cap (default `1`) is hit and no cached
    /// payload is available to serve in its place. The Phase 3 AV
    /// adapter never emits this variant directly.
    #[error("per-symbol fundamentals budget exhausted for {symbol}")]
    PerSymbolBudgetExhausted { symbol: String },

    /// Catch-all for anything else (transport, unrecognised AV response,
    /// internal coalescing failures). The message is the upstream
    /// `Display` form so log triage stays cheap.
    #[error("{0}")]
    Other(String),
}

/// Async fetch of company fundamentals for `symbol`. Implementations
/// must be `Send + Sync + 'static` so a single `Arc<dyn FundamentalsProvider>`
/// can be `app.manage`'d into Tauri state and shared across the MCP +
/// command surface. Dyn-compatibility is provided by `#[async_trait]`.
///
/// Implementations should treat `symbol` as case-insensitive
/// (uppercased internally) so the `aapl` / `AAPL` distinction never
/// reaches the upstream cache.
#[async_trait]
pub trait FundamentalsProvider: Send + Sync + 'static {
    async fn fetch(&self, symbol: &str) -> Result<FundamentalData, FundamentalsError>;
}
