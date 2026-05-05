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

use crate::events::EventEmitter;
use crate::mcp::ibkr_seam::AccountReader;
use crate::services::auto_scanner::{AutoScannerService, MarketScanner};
use crate::services::candidate_promoter::CandidatePromoter;
use crate::services::candidate_universe::CandidateUniverseService;
use crate::services::financial_data_service::FinancialDataService;
use crate::services::fundamentals_provider::FundamentalsProvider;
use crate::services::historical_data_service::HistoricalDataService;
use crate::services::llm_service::LlmService;
use crate::services::manual_fundamentals_store::ManualFundamentalsStore;
use crate::services::news_provider::NewsProvider;
use crate::services::quote_service::QuoteService;
use crate::services::social_sentiment::SocialSentimentService;
use crate::services::ticker_primer::TickerPrimerService;
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
    /// `tools::news` (`news_cache::read_cache_with_verdict(&Db, ...)`).
    pub(crate) db: Arc<Db>,
    /// Retained for the residual fundamentals AV-cache invalidation
    /// hook used by `tools::set_fundamentals`. The news path no longer
    /// reaches into `FinancialDataService` — see [`Self::news_provider`].
    pub(crate) financial_service: Arc<FinancialDataService>,
    /// Used by `tools::news` for the best-effort upstream refresh path
    /// when the cache is missing or stale. Production wires
    /// [`crate::services::news_provider::ibkr::IbkrNewsProvider`].
    pub(crate) news_provider: Arc<dyn NewsProvider>,
    /// Used by `tools::fundamentals` for `fetch(symbol)`. Phase 3 wires
    /// the AV adapter directly; Phase 4 swaps in the composite (manual
    /// store → AV cache → AV API) without touching this field's type.
    pub(crate) fundamentals_provider: Arc<dyn FundamentalsProvider>,
    /// Used by `tools::set_fundamentals` (Phase 4) to persist operator-
    /// curated rows that the composite provider reads ahead of AV.
    pub(crate) manual_fundamentals: Arc<ManualFundamentalsStore>,
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
    /// Used by Phase-02 write tools to broadcast `AppEvent::*Written`
    /// after a successful mutation so the React UI can re-query the
    /// affected slice without polling.
    pub(crate) emitter: Arc<EventEmitter>,
    /// Used by `tools::get_sentiment` (Phase 3) to read durable
    /// `social_sentiment` snapshots. Read-only path — refreshes are
    /// driven by the in-app `SocialSentimentScheduler`.
    pub(crate) social_sentiment: Arc<SocialSentimentService>,
    /// Used by `tools::get_candidates` (Phase 4) for the agent's
    /// candidate-universe inbox. Read-only — auto-scanner +
    /// sentiment-surge populate the rows.
    pub(crate) candidates: Arc<CandidateUniverseService>,
    /// Used by `tools::promote_candidate` (Phase 4) to move a
    /// candidate row into the live `tracked_tickers` watchlist.
    pub(crate) candidate_promoter: Arc<CandidatePromoter>,
    /// Used by `tools::add_ticker` to spawn the post-add prime chain
    /// (fundamentals → projection cache → news). Fire-and-forget; the
    /// MCP response returns the moment the row is inserted, and the
    /// primer emits `AppEvent::TickerPrimingDone` when the chain
    /// completes.
    pub(crate) primer: Arc<TickerPrimerService>,
    /// Caller identity stamped into `mcp_audit.caller` and
    /// `research_notes.written_by` for every write tool invocation. v1
    /// uses a single value per server instance — `"interactive"` for
    /// the live Tauri-hosted server, `"agent_<loop>"` for headless
    /// agent loops. Per-connection caller resolution is a future
    /// enhancement once the agent loops land in Phase 5/6.
    pub(crate) caller: String,
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

    #[allow(clippy::too_many_arguments)] // 15 Arcs — see module docs; one
                                         // Arc per service the tools touch.
                                         // Grouping them buys nothing.
    pub fn new(
        llm: Arc<LlmService>,
        tracker: Arc<TrackerService>,
        db: Arc<Db>,
        financial_service: Arc<FinancialDataService>,
        fundamentals_provider: Arc<dyn FundamentalsProvider>,
        manual_fundamentals: Arc<ManualFundamentalsStore>,
        news_provider: Arc<dyn NewsProvider>,
        historical_service: Arc<HistoricalDataService>,
        quote_service: Arc<QuoteService>,
        ibkr_client: Arc<dyn AccountReader>,
        auto_scanner: Arc<AutoScannerService>,
        market_scanner: Arc<dyn MarketScanner>,
        emitter: Arc<EventEmitter>,
        social_sentiment: Arc<SocialSentimentService>,
        candidates: Arc<CandidateUniverseService>,
        candidate_promoter: Arc<CandidatePromoter>,
        primer: Arc<TickerPrimerService>,
        caller: String,
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
            + Self::set_fundamentals_router()
            + Self::quote_router()
            + Self::positions_router()
            + Self::account_summary_router()
            + Self::executions_router()
            + Self::get_today_playbook_router()
            + Self::get_trade_legs_router()
            + Self::get_trade_review_router()
            + Self::get_trader_profile_router()
            + Self::get_watchlist_briefing_router()
            + Self::scanner_router()
            + Self::add_ticker_router()
            + Self::archive_ticker_router()
            + Self::write_research_note_router()
            + Self::write_morning_pack_router()
            + Self::write_playbook_router()
            + Self::write_trade_review_router()
            + Self::ack_alert_router()
            + Self::mark_alert_enriched_router()
            + Self::get_sentiment_router()
            + Self::get_candidates_router()
            + Self::promote_candidate_router()
            + Self::get_morning_pack_router()
            + Self::get_outcomes_router()
            + Self::get_calibration_stats_router()
            + Self::get_prediction_history_router()
            + Self::get_cost_attribution_router()
            + Self::append_journal_entry_router();
        Self {
            llm,
            tracker,
            db,
            financial_service,
            fundamentals_provider,
            manual_fundamentals,
            news_provider,
            historical_service,
            quote_service,
            ibkr_client,
            auto_scanner,
            market_scanner,
            emitter,
            social_sentiment,
            candidates,
            candidate_promoter,
            primer,
            caller,
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
