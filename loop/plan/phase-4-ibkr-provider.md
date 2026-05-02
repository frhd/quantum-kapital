# Phase 4 — IBKR Reuters fundamentals provider (real implementation)

> Part of [Alpha Vantage → IBKR Reuters](master.md). See index for invariants.

**Status:** todo

**Depends on:** 2 (need the XML fixtures + crate-path decision), 3 (need the trait to implement)

**Goal:** Implement `IbkrFundamentalsProvider`. Extend the IBKR client trait with `req_fundamental_data`. Write Reuters XML parsers for ReportSnapshot, ReportsFinSummary, ReportsFinStatements, RESC. Map all four into the existing `FundamentalData` shape. Tests use the Phase 2 fixtures, so this phase doesn't require TWS in CI.

## Files

- New: `src-tauri/src/services/fundamentals_provider/ibkr/mod.rs` — `IbkrFundamentalsProvider` impl. Holds `Arc<dyn IbkrClientTrait>` + cache handle.
- New: `src-tauri/src/services/fundamentals_provider/ibkr/xml.rs` — Reuters XML parsers, one function per reportType, returning intermediate strongly-typed structs.
- New: `src-tauri/src/services/fundamentals_provider/ibkr/mapper.rs` — convert intermediate structs to `FundamentalData`. Keeps parsing and mapping testable separately.
- New: `src-tauri/src/services/fundamentals_provider/ibkr/cache.rs` — IBKR-side on-disk cache (raw XML and parsed `FundamentalData` JSON) under `cache/ibkr_fundamentals/`, 7-day TTL via `CacheService`.
- New: `src-tauri/src/services/fundamentals_provider/ibkr/tests.rs` — fixture-based parser + mapper tests; mock-client end-to-end test.
- Touches: `src-tauri/src/ibkr/client/` (specific file driven by Phase 2's crate decision) — add `req_fundamental_data(symbol: &str, report_type: ReportType) -> Result<String /* xml */, IbkrError>` to `IbkrClientTrait` and live impl.
- Touches: `src-tauri/src/ibkr/mocks.rs` — `MockIbkrClient::req_fundamental_data` returns canned XML keyed on `(symbol, report_type)`. Loaded from Phase 2 fixtures via `include_str!`.
- Touches: `src-tauri/src/ibkr/error.rs` — add `MarketDataSubscriptionDenied` variant (or equivalent) so `FundamentalsError::NoSubscription` has a clean source.
- Touches: `src-tauri/src/lib.rs` — construct `IbkrFundamentalsProvider` (not yet wired as default; Phase 5 flips the default).

## Reuse

- `services/cache_service.rs::CacheService` — same 7-day TTL pattern used by AV.
- `ibkr/mocks.rs::MockIbkrClient` — extend, don't replace.
- `IbkrError` taxonomy — extend with subscription-denial variant if not present.
- Phase 2 fixtures at `src-tauri/tests/fixtures/ibkr_fundamentals/`.
- The existing `FundamentalData` shape (no changes).
- `services/fundamentals_provider/test_support.rs::FakeFundamentalsProvider` from Phase 3 for any downstream tests that don't care which provider supplies the data.

## Decisions to make in this phase

- **XML parser library.** `quick-xml` (event-based, fast) vs. `serde-xml-rs` (declarative, slower). Default: `quick-xml` — Reuters XML is large enough that streaming pays for itself.
- **Cache key shape.** `<SYMBOL>_<reportType>` (4 entries per symbol) vs. `<SYMBOL>_full` (1 entry). Default: per-reportType, so partial fetches succeed and TTL can vary by report.
- **Concurrent reportType fetches.** Mirror AV's `tokio::try_join!` for parallel fetch, OR sequential to respect TWS pacing? Default: sequential with ~200ms spacing. TWS pacing is stricter than AV's was.
- **Required vs. optional reportTypes.** ReportSnapshot is required (mkt cap, ratios). RESC (analyst estimates) might be unsubscribed for some account tiers. Default: ReportSnapshot + ReportsFinSummary required; ReportsFinStatements + RESC optional, missing fields surface as `None` in `FundamentalData`.
- **Field-level fallbacks.** What if revenue is missing from one reportType but present in another? Default: each `FundamentalData` field has a primary source, no cross-source synthesis (keeps debugging simple).

## Exit criteria

- All Phase 2 fixtures parse without error; parsed values match hand-verified expectations from public AAPL fundamentals (one snapshot test per reportType).
- `IbkrFundamentalsProvider::fetch("AAPL")` against `MockIbkrClient` (loaded from fixtures) returns a `FundamentalData` whose populated fields are non-zero / non-empty for at least: `current_metrics.market_cap`, `current_metrics.pe_ratio`, last 4 quarters of `historical[].revenue`, last 4 quarters of `historical[].eps`.
- New test: provider against a mock that returns the TWS subscription-denied error → `FundamentalsError::NoSubscription`.
- New test: provider against a mock that returns malformed XML → `FundamentalsError::ParseError(...)`.
- New test: RESC unsubscribed but other reportTypes succeed → provider returns `FundamentalData` with empty `analyst_estimates` and emits a `warn!`. Does not error.
- Cache test: two consecutive `fetch("AAPL")` calls produce one `req_fundamental_data` set on the live client (the second is a cache hit).
- `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check` clean. Pre-commit clean.
- The Phase 2 spike binary is deleted (or moved behind `#[cfg(feature = "ibkr-spike")]`); fixtures stay.

## Gotchas

- **TWS error semantics.** `req_fundamental_data` returns errors via the standard TWS error message channel, not as a return value. The trait needs to map TWS error code 430 (no subscription) and 200 (no security definition) to typed errors. Exact codes come from the Phase 2 spike notes.
- **`reportType` enum.** Use a Rust enum (`ReportType::Snapshot | FinSummary | FinStatements | Resc`) at the trait boundary; convert to TWS string at the wire boundary. Don't pass strings around.
- **XML element names sometimes drift across symbols.** Phase 2 should have flagged this; if not, the AAPL-only fixture set may not catch all field-position variants. Add at least one non-AAPL fixture per reportType before claiming the parsers are robust.
- **Cache invalidation on schema change.** If you change the parser, invalidate the cache (e.g., bump a `cache_version` constant included in the cache key). Otherwise stale parsed JSON survives the parser fix.
- **Subscription denial may be partial.** RESC (analyst estimates) is a separate sub from the rest. Provider must succeed when RESC fails but the others work — populate `analyst_estimates` with `Default::default()` and warn-log, do not error.
- **Live impl is not unit-tested.** The trait extension's live `req_fundamental_data` is integration-tested only (against a paper TWS in a manual or feature-gated test). Fixtures cover the parser; nothing covers the wire format end-to-end without TWS.
