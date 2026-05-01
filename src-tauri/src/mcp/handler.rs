//! Phase 1 — read-only MCP server handler.
//!
//! Hosts the `rmcp` `ServerHandler` and the composed [`ToolRouter`] that
//! aggregates every per-tool `#[tool_router]` block declared under
//! `mcp/tools/`. Each tool lives in its own file (one `impl McpHandler`
//! block per file with a uniquely-named router) and is mounted here via
//! `ToolRouter`'s `Add` impl. This keeps `handler.rs` lean as the
//! surveillance tool surface grows; see `mcp/tools/mod.rs` for the file
//! layout. Tools are read-only by construction — see the
//! surveillance-only rule in the workspace `CLAUDE.md`.

use std::sync::Arc;

use rmcp::{
    handler::server::router::tool::ToolRouter, tool_handler, ErrorData as McpError, ServerHandler,
};

use crate::mcp::ibkr_seam::AccountReader;
use crate::services::auto_scanner::{AutoScannerService, MarketScanner};
use crate::services::financial_data_service::FinancialDataService;
use crate::services::historical_data_service::HistoricalDataService;
use crate::services::llm_service::LlmService;
use crate::services::quote_service::QuoteService;
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;

/// rmcp server handler. One instance per running MCP server.
///
/// Holds an `Arc` for every service the registered tools touch. Adding a
/// new tool is usually: (1) drop a new file under `mcp/tools/`, (2) add
/// `+ Self::<new_router>()` to the router composition in `new`, and
/// (3) declare the module in `mcp/tools/mod.rs`.
#[derive(Clone)]
pub struct McpHandler {
    /// Used by `tools::budget` (LLM spend / budget reporting).
    pub(crate) llm: Arc<LlmService>,
    /// Used by `tools::watchlist` and `tools::setups`.
    pub(crate) tracker: Arc<TrackerService>,
    /// Used by `tools::alerts` (`list_alerts(&Arc<Db>, ...)`) and
    /// `tools::news` (`read_cache_with_verdict(&Db, ...)`).
    pub(crate) db: Arc<Db>,
    /// Used by `tools::news` for the best-effort AV refresh path and by
    /// `tools::fundamentals` for `fetch_fundamental_data`.
    pub(crate) financial_service: Arc<FinancialDataService>,
    /// Used by `tools::bars` (`fetch_bars` — cache-first, IBKR fallback).
    pub(crate) historical_service: Arc<HistoricalDataService>,
    /// Used by `tools::quote` (live IBKR snapshot, never cached).
    pub(crate) quote_service: Arc<QuoteService>,
    /// Used by `tools::positions` and `tools::account_summary`. The
    /// narrow `AccountReader` trait (rather than the concrete
    /// `IbkrClient`) means tests can plug `MockIbkrClient` without a
    /// live TWS — mirroring the `MarketScanner` / `QuoteFetcher`
    /// pattern used elsewhere in the codebase.
    pub(crate) ibkr_client: Arc<dyn AccountReader>,
    /// Used by `tools::scanner` to look up `ScanProfile` by name. The
    /// scan itself goes through `market_scanner` so the trait seam stays
    /// narrow.
    pub(crate) auto_scanner: Arc<AutoScannerService>,
    /// Used by `tools::scanner` for the actual `scan(subscription)` call.
    pub(crate) market_scanner: Arc<dyn MarketScanner>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl McpHandler {
    /// Names of every tool registered on the composed [`ToolRouter`].
    ///
    /// Used by the surveillance-only audit (`tests/mcp_surveillance_audit.rs`)
    /// to enforce that the MCP surface never grows an order-placement
    /// primitive — see the workspace `CLAUDE.md`. `pub` (not `pub(crate)`)
    /// because integration tests under `tests/*.rs` compile against the
    /// library crate's public API only; downstream consumers have no
    /// reason to call this and the audit needs cross-crate visibility.
    pub fn tool_names(&self) -> Vec<String> {
        self.tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    #[allow(clippy::too_many_arguments)] // 9 Arcs — see module docs; one
                                         // Arc per service the tools touch.
                                         // Grouping them buys nothing.
    pub fn new(
        llm: Arc<LlmService>,
        tracker: Arc<TrackerService>,
        db: Arc<Db>,
        financial_service: Arc<FinancialDataService>,
        historical_service: Arc<HistoricalDataService>,
        quote_service: Arc<QuoteService>,
        ibkr_client: Arc<dyn AccountReader>,
        auto_scanner: Arc<AutoScannerService>,
        market_scanner: Arc<dyn MarketScanner>,
    ) -> Self {
        // Each per-tool file declares its own `#[tool_router(router = X_router)]`
        // block; `ToolRouter` composes via `+`. Adding a tool means: drop a
        // new file with its own router and add it to this sum.
        let tool_router = Self::budget_router()
            + Self::watchlist_router()
            + Self::setups_router()
            + Self::alerts_router()
            + Self::news_router()
            + Self::bars_router()
            + Self::fundamentals_router()
            + Self::quote_router()
            + Self::positions_router()
            + Self::account_summary_router()
            + Self::scanner_router();
        Self {
            llm,
            tracker,
            db,
            financial_service,
            historical_service,
            quote_service,
            ibkr_client,
            auto_scanner,
            market_scanner,
            tool_router,
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpHandler {}

// `test_handler_with_seeded_spend` previously lived here; Step 5 moved it
// to `mcp::tools::test_support` so the per-tool unit tests and the
// cross-crate integration test can share the same constructor.
#[doc(hidden)]
pub use crate::mcp::tools::test_support::test_handler_with_seeded_spend;
