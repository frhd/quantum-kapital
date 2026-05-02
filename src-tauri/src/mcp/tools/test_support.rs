//! Shared test scaffolding for the per-tool unit tests under `mcp/tools`.
//!
//! Lifted out of `handler.rs::tests` once Step 5 split tools into one file
//! per tool — every `tools/*.rs` test now needs the same `Db`-backed
//! `McpHandler` constructor plus a deterministic `LlmClock`.
//!
//! Also re-exports `test_handler_with_seeded_spend`, the `#[doc(hidden)]`
//! constructor consumed by `tests/mcp_tool_call.rs` (the cross-crate
//! integration test). It lives here so the integration test can reach it
//! without the rest of the test scaffolding being public; it intentionally
//! is **not** gated on `#[cfg(test)]` because integration tests compile the
//! library as a regular dependency when targeting `--release` profiles.

#![allow(dead_code)] // each helper is consumed by a subset of the per-tool tests.

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

#[cfg(test)]
use tempfile::NamedTempFile;

use async_trait::async_trait;

use crate::config::settings::AutoScannerConfig;
use crate::events::EventEmitter;
use crate::ibkr::error::{IbkrError, Result as IbkrResult};
use crate::ibkr::types::historical::{HistoricalBar, HistoricalDataRequest};
use crate::ibkr::types::{
    AccountSummary, MarketDataSnapshot, Position, ScannerData, ScannerSubscription,
};
use crate::mcp::handler::McpHandler;
use crate::mcp::ibkr_seam::AccountReader;
use crate::middleware::HistoricalRateLimiter;
use crate::services::auto_scanner::{AutoScannerService, MarketScanner};
use crate::services::candidate_promoter::CandidatePromoter;
use crate::services::candidate_universe::CandidateUniverseService;
use crate::services::financial_data_service::FinancialDataService;
use crate::services::fundamentals_provider::test_support::FakeFundamentalsProvider;
use crate::services::fundamentals_provider::FundamentalsProvider;
use crate::services::historical_data_service::{HistoricalDataFetcher, HistoricalDataService};
use crate::services::llm_service::{LlmClock, LlmService};
use crate::services::quote_service::{QuoteFetcher, QuoteService};
use crate::services::social_sentiment::SocialSentimentService;
use crate::services::tracker_service::TrackerService;
use crate::storage::Db;

#[cfg(test)]
use crate::ibkr::mocks::MockIbkrClient;

/// Stub fetcher that panics if its `fetch_historical` is called. Used by
/// tests that pre-seed `bars_cache` and want any cache-miss path to surface
/// as a loud test failure instead of a hang or a real network attempt.
pub struct PanickingFetcher;

/// No-op IBKR seams used by `test_handler_with_seeded_spend` — the
/// cross-crate integration test only exercises `get_llm_budget_status`,
/// so the four IBKR-touching Arcs need a stub that satisfies the type
/// system without pulling in the full `MockIbkrClient` (which is
/// `#[cfg(test)]`-gated and therefore invisible to integration tests).
///
/// Every method returns `IbkrError::NotConnected`. If a future
/// integration test wants to drive a live-IBKR tool through this
/// handler, add a richer stub or have that test use a `MockIbkrClient`
/// directly via the unit-test helpers.
pub struct NotConnectedStub;

#[async_trait]
impl AccountReader for NotConnectedStub {
    async fn list_accounts(&self) -> IbkrResult<Vec<String>> {
        Err(IbkrError::NotConnected)
    }
    async fn get_positions(&self, _account: &str) -> IbkrResult<Vec<Position>> {
        Err(IbkrError::NotConnected)
    }
    async fn get_account_summary(&self, _account: &str) -> IbkrResult<Vec<AccountSummary>> {
        Err(IbkrError::NotConnected)
    }
}

#[async_trait]
impl QuoteFetcher for NotConnectedStub {
    async fn get_market_data_snapshot(&self, _symbol: &str) -> IbkrResult<MarketDataSnapshot> {
        Err(IbkrError::NotConnected)
    }
}

#[async_trait]
impl MarketScanner for NotConnectedStub {
    async fn scan(&self, _subscription: ScannerSubscription) -> IbkrResult<Vec<ScannerData>> {
        Err(IbkrError::NotConnected)
    }
}

#[async_trait]
impl HistoricalDataFetcher for PanickingFetcher {
    async fn fetch_historical(
        &self,
        request: HistoricalDataRequest,
    ) -> IbkrResult<Vec<HistoricalBar>> {
        panic!(
            "PanickingFetcher: cache-miss fetch attempted in test (request: {:?})",
            request
        );
    }
}

/// Deterministic [`LlmClock`] fixed at construction time. Lets budget /
/// spend tests pin "today" without leaking real wall-clock behaviour.
pub struct FixedClock(pub AtomicI64);

impl LlmClock for FixedClock {
    fn now_unix(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Open a fresh on-disk SQLite Db for a single test. The `NamedTempFile`
/// is returned alongside so the caller keeps it alive for the test's
/// duration — dropping it deletes the underlying file.
#[cfg(test)]
pub fn make_db() -> (NamedTempFile, Arc<Db>) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path()).expect("open db");
    (tmp, Arc::new(db))
}

/// Build an `McpHandler` wired to fresh services on top of the supplied
/// `Db`. Used by the data-tier tools (watchlist / setups / alerts / news /
/// bars / fundamentals) which don't care about the LLM budget; the
/// `LlmService` is constructed with a no-op API key and a generous budget.
///
/// The historical-data service is wired with [`PanickingFetcher`] so any
/// cache-miss path raises a loud test failure rather than hanging or
/// attempting a real network round-trip. Tests that need to exercise a
/// fetch path should construct their own service.
#[cfg(test)]
pub fn handler_for_db(db: Arc<Db>) -> McpHandler {
    let mock = Arc::new(MockIbkrClient::new());
    build_handler(db, mock)
}

/// Build an `McpHandler` whose IBKR-touching services all delegate to the
/// supplied `MockIbkrClient`. Used by Step 7 live-tool tests so a single
/// mock instance can program quotes, positions, account summaries, and
/// scanner results behind every tool that hits a live-IBKR seam.
///
/// The mock is connected automatically — every `IbkrClientTrait` /
/// `QuoteFetcher` / `MarketScanner` method on `MockIbkrClient` short-
/// circuits with `IbkrError::NotConnected` otherwise, which would mask
/// the real assertion the test is trying to make.
#[cfg(test)]
pub async fn handler_for_mock_ibkr(db: Arc<Db>, mock: Arc<MockIbkrClient>) -> McpHandler {
    mock.set_connected(true).await;
    build_handler(db, mock)
}

/// Build an `McpHandler` from a caller-supplied `LlmService`. Used by
/// the budget tests, which seed a deterministic spend-and-clock state
/// before the handler is built.
#[cfg(test)]
pub fn handler_with_llm(db: Arc<Db>, llm: Arc<LlmService>) -> McpHandler {
    let mock = Arc::new(MockIbkrClient::new());
    build_handler_with_llm(db, mock, llm)
}

/// Inner constructor. Synchronous so `handler_for_db` (used by every
/// pre-Step-7 test) keeps its non-async signature; the async wrapper
/// `handler_for_mock_ibkr` adds the connect step the live-IBKR tools
/// require.
#[cfg(test)]
fn build_handler(db: Arc<Db>, mock: Arc<MockIbkrClient>) -> McpHandler {
    let llm = Arc::new(LlmService::new(
        "test-key".to_string(),
        Arc::clone(&db),
        100.0,
    ));
    build_handler_with_llm(db, mock, llm)
}

#[cfg(test)]
fn build_handler_with_llm(
    db: Arc<Db>,
    mock: Arc<MockIbkrClient>,
    llm: Arc<LlmService>,
) -> McpHandler {
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    // Empty API key + empty base URL — every news fetch falls through to
    // the cache-only path. Tests that exercise the AV-fallback branch
    // override this by calling `FinancialDataService::new(...)` themselves.
    let financial = Arc::new(FinancialDataService::new(String::new()).with_db(Arc::clone(&db)));
    let fetcher: Arc<dyn HistoricalDataFetcher> = Arc::new(PanickingFetcher);
    let hist = Arc::new(HistoricalDataService::new(
        Arc::clone(&db),
        fetcher,
        Arc::new(HistoricalRateLimiter::new(60)),
    ));

    // Three Arc<dyn _> coercions of the same MockIbkrClient — each tool
    // imports a different trait but they all delegate to the same mock
    // state (set_positions, set_scan_results, etc.).
    let quote_fetcher: Arc<dyn QuoteFetcher> = Arc::clone(&mock) as Arc<dyn QuoteFetcher>;
    let quote = Arc::new(QuoteService::new(quote_fetcher));
    let ibkr_client: Arc<dyn AccountReader> = Arc::clone(&mock) as Arc<dyn AccountReader>;
    let market_scanner: Arc<dyn MarketScanner> = Arc::clone(&mock) as Arc<dyn MarketScanner>;
    let candidates = Arc::new(CandidateUniverseService::new(Arc::clone(&db)));
    let promoter = Arc::new(CandidatePromoter::new(
        Arc::clone(&candidates),
        Arc::clone(&tracker),
        0.0,
    ));
    let auto_scanner = Arc::new(AutoScannerService::new(
        Arc::clone(&market_scanner),
        Arc::clone(&tracker),
        Arc::clone(&promoter),
        Arc::clone(&db),
        AutoScannerConfig::default(),
    ));

    let emitter = Arc::new(EventEmitter::for_capture());
    // No providers wired — `get_sentiment` only reads `social_sentiment`,
    // and any provider-needing test seeds rows directly via `repo`.
    let social_sentiment = Arc::new(SocialSentimentService::new(Arc::clone(&db), Vec::new()));
    // Empty fundamentals provider — every `fetch` returns `NotFound`, so
    // `get_fundamentals` tests inheriting this builder land on the
    // existing "domain error" assertion path. Tests that need a hit
    // pre-load via `FakeFundamentalsProvider::insert` and pass an
    // explicit handler.
    let fundamentals_provider: Arc<dyn FundamentalsProvider> =
        Arc::new(FakeFundamentalsProvider::new());
    McpHandler::new(
        llm,
        tracker,
        db,
        financial,
        fundamentals_provider,
        hist,
        quote,
        ibkr_client,
        auto_scanner,
        market_scanner,
        emitter,
        social_sentiment,
        candidates,
        promoter,
        "interactive".to_string(),
    )
}

/// Construct an `McpHandler` against a fresh on-disk DB at `db_path` with
/// a single seeded `llm_calls` row representing today's spend. The clock
/// is fixed at `2023-11-14 22:13:20 UTC`.
///
/// Lives here so the cross-crate integration test in
/// `tests/mcp_tool_call.rs` can construct a realistic handler without
/// pulling private internals of `LlmService` / `Db` / the test clock.
#[doc(hidden)]
pub async fn test_handler_with_seeded_spend(
    db_path: &Path,
    spent_today_usd: f64,
    daily_budget_usd: f64,
) -> std::io::Result<McpHandler> {
    let db =
        Arc::new(Db::open(db_path).map_err(|e| std::io::Error::other(format!("open db: {e}")))?);

    // 2023-11-14 22:13:20 UTC — well after that day's UTC midnight.
    let now: i64 = 1_700_000_000;
    let day_start: i64 = (now / 86_400) * 86_400;

    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO llm_calls (kind, model, input_tokens, output_tokens, \
             cache_read_tokens, cost_usd, called_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "thesis",
                "claude-sonnet-4-6",
                0i64,
                0i64,
                0i64,
                spent_today_usd,
                day_start
            ],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("seed llm_calls: {e}")))?;

    let clock: Arc<dyn LlmClock> = Arc::new(FixedClock(AtomicI64::new(now)));
    let llm = Arc::new(
        LlmService::new("test-key".to_string(), Arc::clone(&db), daily_budget_usd)
            .with_clock(clock),
    );
    let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
    let financial = Arc::new(FinancialDataService::new(String::new()).with_db(Arc::clone(&db)));
    let fetcher: Arc<dyn HistoricalDataFetcher> = Arc::new(PanickingFetcher);
    let hist = Arc::new(HistoricalDataService::new(
        Arc::clone(&db),
        fetcher,
        Arc::new(HistoricalRateLimiter::new(60)),
    ));
    // The integration test only exercises `get_llm_budget_status` — none
    // of the live-IBKR tools — so disconnected stubs are fine. Wired
    // through every Arc the tools need so the handler still builds.
    let stub = Arc::new(NotConnectedStub);
    let quote_fetcher: Arc<dyn QuoteFetcher> = Arc::clone(&stub) as Arc<dyn QuoteFetcher>;
    let quote = Arc::new(QuoteService::new(quote_fetcher));
    let ibkr_client: Arc<dyn AccountReader> = Arc::clone(&stub) as Arc<dyn AccountReader>;
    let market_scanner: Arc<dyn MarketScanner> = Arc::clone(&stub) as Arc<dyn MarketScanner>;
    let candidates = Arc::new(CandidateUniverseService::new(Arc::clone(&db)));
    let promoter = Arc::new(CandidatePromoter::new(
        Arc::clone(&candidates),
        Arc::clone(&tracker),
        0.0,
    ));
    let auto_scanner = Arc::new(AutoScannerService::new(
        Arc::clone(&market_scanner),
        Arc::clone(&tracker),
        Arc::clone(&promoter),
        Arc::clone(&db),
        AutoScannerConfig::default(),
    ));
    let emitter = Arc::new(EventEmitter::for_capture());
    let social_sentiment = Arc::new(SocialSentimentService::new(Arc::clone(&db), Vec::new()));
    let fundamentals_provider: Arc<dyn FundamentalsProvider> =
        Arc::new(FakeFundamentalsProvider::new());
    Ok(McpHandler::new(
        llm,
        tracker,
        db,
        financial,
        fundamentals_provider,
        hist,
        quote,
        ibkr_client,
        auto_scanner,
        market_scanner,
        emitter,
        social_sentiment,
        candidates,
        promoter,
        "interactive".to_string(),
    ))
}
