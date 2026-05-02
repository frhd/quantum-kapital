//! [`CompositeFundamentalsProvider`] — the production
//! [`FundamentalsProvider`] wired into `lib.rs`. Reads in order:
//!
//! 1. **Manual store.** Operator-curated rows written by the MCP
//!    `set_fundamentals` tool. Always wins (Hard Invariant #8).
//! 2. **AV cache short-circuit.** When all three AV file-cache rows
//!    (`{SYMBOL}_overview` / `_income` / `_earnings`) are fresh, the
//!    composite delegates to the AV provider without touching the
//!    ledger — the AV provider serves from cache and no wire call
//!    fires. This keeps "user re-opens the analysis screen" cheap.
//! 3. **Ledger-gated AV adapter.** When the AV cache is missing or
//!    stale, the [`super::av_call_ledger::AvCallLedger`] gates the AV
//!    call: per-symbol cap → daily hard cap → soft-cap warn → AV
//!    fetch → commit-on-success. On cap exhaustion, the composite
//!    serves stale cached data when available; otherwise surfaces a
//!    typed [`FundamentalsError::DailyBudgetExhausted`] /
//!    [`FundamentalsError::PerSymbolBudgetExhausted`].
//!
//! Hard Invariant #6 (the tracker doesn't fetch fundamentals) is
//! preserved: this provider only adds layering — every consumer is still
//! a user-explicit code path (analysis UI, MCP tools).

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, warn};

use crate::ibkr::types::FundamentalData;
use crate::services::cache_service::CacheService;

use super::av_call_ledger::{AvCallLedger, AvLedgerError, ReserveOutcome};
use super::manual::ManualFundamentalsProvider;
use super::{FundamentalsError, FundamentalsProvider};

/// AV-side guardrail bundle: the ledger gates wire-calls and the cache
/// service is consulted to decide whether a call is wire-bound or
/// cache-served. Both halves are required — the composite cannot
/// distinguish cache-served calls from wire-bound calls without the
/// cache, and cannot enforce the budget without the ledger.
pub struct AvGuard {
    pub ledger: Arc<AvCallLedger>,
    pub cache: Arc<CacheService>,
}

impl AvGuard {
    pub fn new(ledger: Arc<AvCallLedger>, cache: Arc<CacheService>) -> Self {
        Self { ledger, cache }
    }
}

/// Provider composition: manual store first, AV second. The optional
/// [`AvGuard`] enforces the daily / per-symbol caps on the AV branch
/// when present; tests that want pure provider-trait composition skip
/// it via [`Self::new`].
pub struct CompositeFundamentalsProvider {
    manual: Arc<ManualFundamentalsProvider>,
    av: Arc<dyn FundamentalsProvider>,
    av_guard: Option<Arc<AvGuard>>,
}

impl CompositeFundamentalsProvider {
    /// Build a composite without AV-side guardrails. Used by tests that
    /// only care about the manual-vs-AV layering. Production wires the
    /// guard via [`Self::with_av_guard`].
    pub fn new(manual: Arc<ManualFundamentalsProvider>, av: Arc<dyn FundamentalsProvider>) -> Self {
        Self {
            manual,
            av,
            av_guard: None,
        }
    }

    /// Attach the AV-side guardrails. Production wires this via the
    /// app's `AvCallLedger` + the `FinancialDataService`'s shared
    /// `CacheService`.
    pub fn with_av_guard(mut self, guard: Arc<AvGuard>) -> Self {
        self.av_guard = Some(guard);
        self
    }
}

#[async_trait]
impl FundamentalsProvider for CompositeFundamentalsProvider {
    async fn fetch(&self, symbol: &str) -> Result<FundamentalData, FundamentalsError> {
        match self.manual.fetch(symbol).await {
            Ok(data) => {
                info!("fundamentals(manual): served {symbol} from operator-curated manual store");
                return Ok(data);
            }
            Err(FundamentalsError::NotFound(_)) => {
                // Fall through to the AV branch below.
            }
            Err(e) => {
                // Manual store I/O blew up — distinct from "no row".
                // Don't paper over with the AV fallback because we'd
                // mask a SQLite/serde bug; surface the error and let
                // the operator triage.
                warn!("fundamentals(manual): store read failed for {symbol}: {e}");
                return Err(e);
            }
        }

        let key = symbol.trim().to_uppercase();

        let Some(guard) = self.av_guard.as_ref() else {
            // No guard configured — preserve Phase 4 behaviour for
            // tests that don't wire the cache + ledger.
            info!("fundamentals(av): manual store empty for {symbol}, falling through to Alpha Vantage (no guard)");
            return self.av.fetch(symbol).await;
        };

        // Cache short-circuit: if all three AV rows are fresh, the AV
        // provider will serve from cache without touching the wire. We
        // can delegate without consulting the ledger — that's the
        // user-experience win (re-opening the analysis screen for a
        // recently-fetched symbol stays cheap).
        if av_cache_fresh(&guard.cache, &key) {
            info!("fundamentals(av): {key} served from fresh AV file cache (no ledger touch)");
            return self.av.fetch(symbol).await;
        }

        // AV cache is stale or missing — the call will hit the wire.
        // Consult the ledger before delegating.
        match guard.ledger.check(&key).await {
            Ok(ReserveOutcome::BelowSoftCap) => {
                info!("fundamentals(av): ledger ok (below soft cap), fetching {key} from AV");
            }
            Ok(ReserveOutcome::AboveSoftCap) => {
                warn!(
                    soft_cap = guard.ledger.soft_cap(),
                    hard_cap = guard.ledger.hard_cap(),
                    symbol = %key,
                    "AV ledger past soft cap, allowing fetch but flagging trip-wire"
                );
            }
            Err(AvLedgerError::PerSymbolCapReached { symbol, count }) => {
                warn!(symbol = %symbol, count = count, "AV ledger per-symbol cap reached");
                if let Some(stale) = read_stale_fundamentals(&guard.cache, &key) {
                    warn!(
                        "fundamentals(av): per-symbol cap reached for {key}; serving stale cache"
                    );
                    return Ok(stale);
                }
                return Err(FundamentalsError::PerSymbolBudgetExhausted { symbol });
            }
            Err(AvLedgerError::DailyCapReached { hit_count }) => {
                warn!(hit_count = hit_count, "AV ledger daily hard cap reached");
                if let Some(stale) = read_stale_fundamentals(&guard.cache, &key) {
                    warn!(
                        "fundamentals(av): daily cap reached at {hit_count}/{}; serving stale cache for {key}",
                        guard.ledger.hard_cap()
                    );
                    return Ok(stale);
                }
                return Err(FundamentalsError::DailyBudgetExhausted { hit_count });
            }
            Err(AvLedgerError::Storage(e)) => {
                return Err(FundamentalsError::Other(format!(
                    "AV ledger storage failure: {e}"
                )));
            }
        }

        // Ledger said go. Delegate to AV; on success, commit the
        // ledger increment. On failure, the ticket is unburned — a
        // transport error doesn't silently exhaust the daily quota.
        let result = self.av.fetch(symbol).await;
        if result.is_ok() {
            if let Err(e) = guard.ledger.commit(&key).await {
                // Storage failure on commit is loud but non-fatal:
                // the user got their data, but the next call may
                // overshoot the cap by 1. Log and move on.
                warn!(symbol = %key, error = %e, "AV ledger commit failed after successful AV fetch");
            }
        }
        result
    }
}

/// Are all three AV file-cache rows for `symbol_uppercase` valid (within
/// TTL)? Mirrors the cache-key naming used by
/// `services::financial_data_service::fetch_av_function`.
fn av_cache_fresh(cache: &CacheService, symbol_uppercase: &str) -> bool {
    use crate::services::financial_data_service::AV_FUNDAMENTALS_CACHE_SUFFIXES;
    for suffix in AV_FUNDAMENTALS_CACHE_SUFFIXES {
        let key = format!("{symbol_uppercase}_{suffix}");
        if !cache.is_valid(&key) {
            return false;
        }
    }
    true
}

/// Read the three AV file-cache rows ignoring their TTL and
/// reconstruct a `FundamentalData`. Returns `None` if any of the three
/// rows is missing — partial cache reconstruction is intentionally not
/// supported (the AV adapter's stale-cache fallback runs per endpoint
/// and may produce a record with one stale + two fresh; we don't try
/// to replicate that here).
fn read_stale_fundamentals(
    cache: &CacheService,
    symbol_uppercase: &str,
) -> Option<FundamentalData> {
    crate::services::financial_data_service::FinancialDataService::read_cached_fundamentals_ignoring_ttl(
        cache,
        symbol_uppercase,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::{NamedTempFile, TempDir};

    use super::*;
    use crate::ibkr::types::{CurrentMetrics, FundamentalData, HistoricalFinancial};
    use crate::services::cache_service::CacheService;
    use crate::services::fundamentals_provider::av_call_ledger::AvCallLedger;
    use crate::services::fundamentals_provider::test_support::FakeFundamentalsProvider;
    use crate::services::manual_fundamentals_store::ManualFundamentalsStore;
    use crate::storage::Db;

    fn fd(symbol: &str, pe: f64) -> FundamentalData {
        FundamentalData {
            symbol: symbol.to_string(),
            historical: vec![HistoricalFinancial {
                year: 2024,
                revenue: 100.0,
                net_income: 10.0,
                eps: 1.0,
            }],
            analyst_estimates: None,
            current_metrics: CurrentMetrics {
                price: None,
                pe_ratio: pe,
                shares_outstanding: 1_000.0,
                name: None,
                exchange: None,
                market_cap: None,
                dividend_yield: None,
            },
        }
    }

    fn fresh_store() -> (NamedTempFile, Arc<ManualFundamentalsStore>) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, Arc::new(ManualFundamentalsStore::new(db)))
    }

    fn fresh_db() -> (NamedTempFile, Arc<Db>) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, db)
    }

    #[tokio::test]
    async fn empty_manual_falls_through_to_av() {
        let (_tmp, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let av = FakeFundamentalsProvider::new();
        av.insert("AAPL", fd("AAPL", 30.0));
        let av: Arc<dyn FundamentalsProvider> = Arc::new(av);
        let composite = CompositeFundamentalsProvider::new(manual, av);
        let got = composite.fetch("AAPL").await.unwrap();
        assert_eq!(got.current_metrics.pe_ratio, 30.0);
    }

    #[tokio::test]
    async fn manual_row_wins_over_av() {
        let (_tmp, store) = fresh_store();
        store
            .upsert(
                "AAPL",
                fd("AAPL", 99.0),
                "2026-05-02",
                "src",
                "interactive",
                0,
            )
            .await
            .unwrap();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let av_fake = FakeFundamentalsProvider::new();
        av_fake.insert("AAPL", fd("AAPL", 30.0));
        let av: Arc<dyn FundamentalsProvider> = Arc::new(av_fake);
        let composite = CompositeFundamentalsProvider::new(manual, av);
        let got = composite.fetch("AAPL").await.unwrap();
        assert_eq!(
            got.current_metrics.pe_ratio, 99.0,
            "manual store must win over AV"
        );
    }

    #[tokio::test]
    async fn empty_manual_propagates_av_not_found() {
        let (_tmp, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let av: Arc<dyn FundamentalsProvider> = Arc::new(FakeFundamentalsProvider::new());
        let composite = CompositeFundamentalsProvider::new(manual, av);
        let err = composite.fetch("NOPE").await.expect_err("must error");
        assert!(matches!(err, FundamentalsError::NotFound(_)));
    }

    #[tokio::test]
    async fn empty_manual_propagates_av_other_errors_unchanged() {
        let (_tmp, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let av_fake = FakeFundamentalsProvider::new();
        av_fake.fail_with("AV transport blew up");
        let av: Arc<dyn FundamentalsProvider> = Arc::new(av_fake);
        let composite = CompositeFundamentalsProvider::new(manual, av);
        let err = composite.fetch("AAPL").await.expect_err("must error");
        let msg = err.to_string();
        assert!(msg.contains("AV transport blew up"), "got: {msg}");
    }

    #[tokio::test]
    async fn manual_overwrite_changes_returned_value() {
        let (_tmp, store) = fresh_store();
        store
            .upsert(
                "MSFT",
                fd("MSFT", 25.0),
                "2026-04-01",
                "v1",
                "interactive",
                0,
            )
            .await
            .unwrap();
        let manual = Arc::new(ManualFundamentalsProvider::new(Arc::clone(&store)));
        let av: Arc<dyn FundamentalsProvider> = Arc::new(FakeFundamentalsProvider::new());
        let composite = CompositeFundamentalsProvider::new(manual, av);
        assert_eq!(
            composite
                .fetch("MSFT")
                .await
                .unwrap()
                .current_metrics
                .pe_ratio,
            25.0
        );
        store
            .upsert(
                "MSFT",
                fd("MSFT", 35.0),
                "2026-05-02",
                "v2",
                "interactive",
                1,
            )
            .await
            .unwrap();
        assert_eq!(
            composite
                .fetch("MSFT")
                .await
                .unwrap()
                .current_metrics
                .pe_ratio,
            35.0,
        );
    }

    // ---------- AV-guard tests (Phase 5 cutover) ----------

    fn cache_with_dir() -> (TempDir, Arc<CacheService>) {
        let tmp = TempDir::new().unwrap();
        let cache = Arc::new(CacheService::new(tmp.path()).unwrap());
        (tmp, cache)
    }

    /// Counter-fake AV. Counts every fetch() invocation; used to
    /// assert the ledger short-circuited the call (or didn't).
    struct CountingFake {
        rows: std::sync::Mutex<std::collections::HashMap<String, FundamentalData>>,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl CountingFake {
        fn new() -> Self {
            Self {
                rows: std::sync::Mutex::new(Default::default()),
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn insert(&self, symbol: &str, data: FundamentalData) {
            self.rows
                .lock()
                .unwrap()
                .insert(symbol.to_uppercase(), data);
        }

        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl FundamentalsProvider for CountingFake {
        async fn fetch(&self, symbol: &str) -> Result<FundamentalData, FundamentalsError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let key = symbol.trim().to_uppercase();
            self.rows
                .lock()
                .unwrap()
                .get(&key)
                .cloned()
                .ok_or(FundamentalsError::NotFound(key))
        }
    }

    #[tokio::test]
    async fn av_branch_below_soft_cap_calls_av_and_increments_ledger() {
        let (_tmp_store, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let (_tmp_db, db) = fresh_db();
        let ledger = Arc::new(AvCallLedger::with_caps(
            db,
            20,
            25,
            5,
            Arc::new(super::super::av_call_ledger::LocalDateSource),
        ));
        let (_tmp_cache, cache) = cache_with_dir();
        let av_fake = Arc::new(CountingFake::new());
        av_fake.insert("AAPL", fd("AAPL", 30.0));
        let av: Arc<dyn FundamentalsProvider> =
            Arc::clone(&av_fake) as Arc<dyn FundamentalsProvider>;
        let guard = Arc::new(AvGuard::new(Arc::clone(&ledger), cache));
        let composite =
            CompositeFundamentalsProvider::new(manual, av).with_av_guard(Arc::clone(&guard));

        let got = composite.fetch("AAPL").await.unwrap();
        assert_eq!(got.current_metrics.pe_ratio, 30.0);
        assert_eq!(av_fake.calls(), 1);
        assert_eq!(ledger.daily_count_today().await.unwrap(), 1);
        assert_eq!(ledger.per_symbol_count_today("AAPL").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn av_branch_returns_typed_error_when_daily_cap_exhausted_and_no_cache() {
        let (_tmp_store, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let (_tmp_db, db) = fresh_db();
        // Pre-populate ledger to hard cap 25/25 by configuring a tiny cap and burning through it.
        let ledger = Arc::new(AvCallLedger::with_caps(
            Arc::clone(&db),
            1,
            2,
            5,
            Arc::new(super::super::av_call_ledger::LocalDateSource),
        ));
        // Burn the daily budget: 2 commits hit the hard cap.
        ledger.check("PRE1").await.unwrap();
        ledger.commit("PRE1").await.unwrap();
        ledger.check("PRE2").await.unwrap();
        ledger.commit("PRE2").await.unwrap();
        let (_tmp_cache, cache) = cache_with_dir();
        let av_fake = Arc::new(CountingFake::new());
        av_fake.insert("AAPL", fd("AAPL", 30.0));
        let av: Arc<dyn FundamentalsProvider> =
            Arc::clone(&av_fake) as Arc<dyn FundamentalsProvider>;
        let guard = Arc::new(AvGuard::new(Arc::clone(&ledger), cache));
        let composite =
            CompositeFundamentalsProvider::new(manual, av).with_av_guard(Arc::clone(&guard));

        let err = composite.fetch("AAPL").await.expect_err("must trip");
        match err {
            FundamentalsError::DailyBudgetExhausted { hit_count } => {
                assert_eq!(hit_count, 2);
            }
            other => panic!("expected DailyBudgetExhausted, got {other:?}"),
        }
        // AV must NOT have been called.
        assert_eq!(av_fake.calls(), 0);
    }

    #[tokio::test]
    async fn av_branch_returns_typed_error_when_per_symbol_cap_exhausted_no_cache() {
        let (_tmp_store, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let (_tmp_db, db) = fresh_db();
        let ledger = Arc::new(AvCallLedger::with_caps(
            Arc::clone(&db),
            20,
            25,
            1,
            Arc::new(super::super::av_call_ledger::LocalDateSource),
        ));
        // Burn the per-symbol cap for AAPL.
        ledger.check("AAPL").await.unwrap();
        ledger.commit("AAPL").await.unwrap();

        let (_tmp_cache, cache) = cache_with_dir();
        let av_fake = Arc::new(CountingFake::new());
        av_fake.insert("AAPL", fd("AAPL", 30.0));
        let av: Arc<dyn FundamentalsProvider> =
            Arc::clone(&av_fake) as Arc<dyn FundamentalsProvider>;
        let guard = Arc::new(AvGuard::new(Arc::clone(&ledger), cache));
        let composite =
            CompositeFundamentalsProvider::new(manual, av).with_av_guard(Arc::clone(&guard));

        let err = composite.fetch("AAPL").await.expect_err("must trip");
        match err {
            FundamentalsError::PerSymbolBudgetExhausted { symbol } => {
                assert_eq!(symbol, "AAPL");
            }
            other => panic!("expected PerSymbolBudgetExhausted, got {other:?}"),
        }
        // AV must NOT have been called.
        assert_eq!(av_fake.calls(), 0);
    }

    #[tokio::test]
    async fn manual_cleared_falls_back_to_av() {
        // Mirrors the master-plan e2e step "Manual store has Y, then
        // clear it → fetch falls back to AV cache, then AV".
        let (_tmp_store, store) = fresh_store();
        store
            .upsert(
                "AAPL",
                fd("AAPL", 99.0),
                "2026-05-02",
                "src",
                "interactive",
                0,
            )
            .await
            .unwrap();
        let manual = Arc::new(ManualFundamentalsProvider::new(Arc::clone(&store)));
        let av_fake = Arc::new(CountingFake::new());
        av_fake.insert("AAPL", fd("AAPL", 30.0));
        let av: Arc<dyn FundamentalsProvider> =
            Arc::clone(&av_fake) as Arc<dyn FundamentalsProvider>;
        let composite = CompositeFundamentalsProvider::new(manual, av);

        // Manual wins.
        assert_eq!(
            composite
                .fetch("AAPL")
                .await
                .unwrap()
                .current_metrics
                .pe_ratio,
            99.0
        );
        assert_eq!(av_fake.calls(), 0);

        // Clear manual; AV now serves.
        store.clear("AAPL").await.unwrap();
        assert_eq!(
            composite
                .fetch("AAPL")
                .await
                .unwrap()
                .current_metrics
                .pe_ratio,
            30.0
        );
        assert_eq!(av_fake.calls(), 1);
    }

    #[tokio::test]
    async fn av_branch_serves_stale_cache_when_daily_cap_exhausted() {
        // Pre-seed the AV cache with payloads that round-trip through
        // FinancialDataService::read_cached_fundamentals_ignoring_ttl,
        // exhaust the daily ledger, and assert the composite returns
        // the stale data instead of the typed error.
        use crate::services::cache_service::CacheService;
        use serde_json::json;
        use tempfile::TempDir;

        let (_tmp_store, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));

        let cache_dir = TempDir::new().unwrap();
        let cache = Arc::new(CacheService::new(cache_dir.path()).unwrap());
        // Plant stale-but-parseable AV cache rows. Production cache
        // keys are `<SYM>_overview`, `<SYM>_income_statement`,
        // `<SYM>_earnings` — see `AV_FUNDAMENTALS_CACHE_SUFFIXES`.
        cache
            .write(
                "AAPL_overview",
                &json!({
                    "Symbol": "AAPL",
                    "Name": "Apple Inc.",
                    "Exchange": "NASDAQ",
                    "MarketCapitalization": "3000000000000",
                    "PERatio": "25.0",
                    "SharesOutstanding": "15000000000",
                    "DividendYield": "0.005"
                }),
            )
            .unwrap();
        cache
            .write(
                "AAPL_income_statement",
                &json!({
                    "symbol": "AAPL",
                    "annualReports": [
                        {"fiscalDateEnding": "2024-09-30", "totalRevenue": "390000000000", "netIncome": "100000000000"}
                    ]
                }),
            )
            .unwrap();
        cache
            .write(
                "AAPL_earnings",
                &json!({
                    "symbol": "AAPL",
                    "annualEarnings": [
                        {"fiscalDateEnding": "2024-09-30", "reportedEPS": "6.5"}
                    ],
                    "quarterlyEarnings": []
                }),
            )
            .unwrap();

        // Make the cache 8 days old (past the 7-day TTL) so the
        // cache-fresh probe returns false but the stale-allowed read
        // still succeeds.
        let entries = std::fs::read_dir(cache_dir.path()).unwrap();
        let stale_time =
            std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 24 * 60 * 60);
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let f = std::fs::File::open(&path).unwrap();
                f.set_modified(stale_time).unwrap();
            }
        }

        let (_tmp_db, db) = fresh_db();
        let ledger = Arc::new(AvCallLedger::with_caps(
            Arc::clone(&db),
            1,
            2,
            5,
            Arc::new(super::super::av_call_ledger::LocalDateSource),
        ));
        // Burn the daily budget: 2 commits hit the hard cap.
        ledger.check("PRE1").await.unwrap();
        ledger.commit("PRE1").await.unwrap();
        ledger.check("PRE2").await.unwrap();
        ledger.commit("PRE2").await.unwrap();

        let av_fake = Arc::new(CountingFake::new());
        let av: Arc<dyn FundamentalsProvider> =
            Arc::clone(&av_fake) as Arc<dyn FundamentalsProvider>;
        let guard = Arc::new(AvGuard::new(Arc::clone(&ledger), Arc::clone(&cache)));
        let composite = CompositeFundamentalsProvider::new(manual, av).with_av_guard(guard);

        let got = composite.fetch("AAPL").await.expect("stale fallback hits");
        assert_eq!(got.symbol, "AAPL");
        assert_eq!(got.current_metrics.pe_ratio, 25.0);
        assert!(!got.historical.is_empty());
        // AV provider was never called — the composite served the
        // stale cache directly.
        assert_eq!(av_fake.calls(), 0);
    }

    #[tokio::test]
    async fn av_branch_does_not_burn_ticket_on_av_fetch_failure() {
        let (_tmp_store, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let (_tmp_db, db) = fresh_db();
        let ledger = Arc::new(AvCallLedger::with_caps(
            db,
            20,
            25,
            5,
            Arc::new(super::super::av_call_ledger::LocalDateSource),
        ));
        let (_tmp_cache, cache) = cache_with_dir();
        // No insert => AV returns NotFound.
        let av_fake = Arc::new(CountingFake::new());
        let av: Arc<dyn FundamentalsProvider> =
            Arc::clone(&av_fake) as Arc<dyn FundamentalsProvider>;
        let guard = Arc::new(AvGuard::new(Arc::clone(&ledger), cache));
        let composite =
            CompositeFundamentalsProvider::new(manual, av).with_av_guard(Arc::clone(&guard));

        let err = composite.fetch("ZZZZ").await.expect_err("must NotFound");
        assert!(matches!(err, FundamentalsError::NotFound(_)));
        // Ledger must remain at zero — AV failure cannot burn a ticket.
        assert_eq!(ledger.daily_count_today().await.unwrap(), 0);
    }
}
