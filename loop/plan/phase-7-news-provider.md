# Phase 7 — News provider trait + IBKR news provider

> Part of [Alpha Vantage → IBKR: Full Vendor Strip-out](master.md). See index for invariants.

**Status:** in-progress (part A landed 2026-05-02 — trait + AV adapter + consumer rewire + `news_source` flag; part B awaits Phase 6 fixtures)

**Depends on:** 3 (mirrors the `FundamentalsProvider` trait pattern), 6 (need fixtures + crate-path decision + sentiment-loss audit)

**Goal:** Introduce a `NewsProvider` trait abstracting `FinancialDataService::fetch_news_sentiment`. Wrap the existing AV news path as `AlphaVantageNewsProvider`. Implement `IbkrNewsProvider` against the Phase 6 fork extensions. Route every news consumer (tracker pipeline, MCP tools, anything that calls `fetch_news_sentiment`) through `Arc<dyn NewsProvider>`. Add a `news_source` settings flag, defaulted to `"alpha_vantage"`. End-state: behavior is byte-identical to today, but flipping the flag swaps the backend.

This phase combines what was two phases on the fundamentals side (trait + AV adapter, then IBKR impl) because news is structurally simpler: one shape (`NewsItem`), no XML, no analyst-estimate split, no "RESC may be unsubscribed" branching.

## Files

- New: `src-tauri/src/services/news_provider/mod.rs` — `NewsProvider` trait + `NewsError` enum.
- New: `src-tauri/src/services/news_provider/alpha_vantage.rs` — `AlphaVantageNewsProvider` wrapping `FinancialDataService::fetch_news_sentiment`.
- New: `src-tauri/src/services/news_provider/ibkr/mod.rs` — `IbkrNewsProvider` impl. Holds `Arc<dyn IbkrClientTrait>`, the `news_cache` SQLite handle, and a per-IBKR-news rate limiter.
- New: `src-tauri/src/services/news_provider/ibkr/parsers.rs` — converts IBKR's structured news payloads (historical-list rows + article-body responses) into `NewsItem`. `sentiment` fields stay `None` here; `NewsInterpreter` fills the per-symbol verdict afterward.
- New: `src-tauri/src/services/news_provider/test_support.rs` — `FakeNewsProvider` for downstream tests.
- New: `src-tauri/src/services/news_provider/tests.rs` — fixture-based parser tests; mock-client end-to-end test; trait round-trip tests.
- Touches: `src-tauri/src/ibkr/client/` — extend `IbkrClientTrait` with `req_news_providers`, `req_historical_news`, `req_news_article` (signatures driven by Phase 6 fork). Live impls call into the forked `ibapi`.
- Touches: `src-tauri/src/ibkr/mocks.rs` — `MockIbkrClient` impls return canned payloads keyed on `(symbol, lookback)`. Loaded from Phase 6 fixtures via `include_str!`.
- Touches: `src-tauri/src/ibkr/error.rs` — add error variants if needed (e.g., `NewsSubscriptionDenied`) so `NewsError::NoSubscription` has a clean source.
- Touches: `src-tauri/src/services/financial_data_service/news.rs` — no functional change here; `AlphaVantageNewsProvider` wraps it. Keep the existing soft-skip-to-stale-cache pattern intact.
- Touches: `src-tauri/src/services/news_interpreter/mod.rs` — confirm it operates only on `news_cache` rows and never reads AV-specific fields. If it reads `overall_sentiment_score` or `ticker_sentiment[]`, those reads need a fallback path because IBKR-sourced rows leave them `None`.
- Touches: every news consumer — replace direct `FinancialDataService::fetch_news_sentiment` calls with `Arc<dyn NewsProvider>` injection. Locations to check: `services/tracker_runner/`, `mcp/tools/news.rs`, `services/thesis_generator/`, anywhere else `fetch_news_sentiment` is grep-able.
- Touches: `src-tauri/src/lib.rs` — construct `AlphaVantageNewsProvider` and `IbkrNewsProvider` (the latter not yet wired as default — Phase 8 flips it). `app.manage(Arc<dyn NewsProvider>)` based on the `news_source` setting.
- Touches: `src-tauri/src/config/settings.rs` — add `news_source: String` field, default `"alpha_vantage"`. Validate at load time (unknown value → warn + fall back to default).
- New: `src-tauri/src/middleware/ibkr_news_rate_limit.rs` — per-IBKR-news rate limiter (token bucket sized to the pacing decided in Phase 6). Mirrors `historical_rate_limit.rs`.
- Touches: `src-tauri/src/middleware/mod.rs` — `pub mod ibkr_news_rate_limit;`.

## Reuse

- The trait pattern Phase 3 established for `FundamentalsProvider` — same structure (trait + error enum + adapter + test_support module).
- Existing `news_cache` SQLite table — both providers write through to it; consumers read from it as before.
- Existing `NewsInterpreter` — vendor-agnostic by design; no rewrite, only an audit per the Phase 6 sentiment-loss notes.
- Phase 6 fixtures at `src-tauri/tests/fixtures/ibkr_news/`.
- `middleware/historical_rate_limit.rs::HistoricalRateLimiter` — pattern.
- The `ibapi` fork from Phase 2, extended in Phase 6.
- Existing `NewsItem` shape — DO NOT change in this phase; it's the contract the trait preserves.

## Decisions to make in this phase

- **`NewsError` variants.** At minimum: `RateLimited { retry_after: Option<Duration> }`, `NoSubscription { provider_code: String }`, `NotConnected`, `ParseError(String)`, `Other(String)`. `NotFound` is debatable — IBKR returns an empty list rather than an error for "no news for this symbol"; treat empty as `Ok(vec![])`, not an error.
- **`fetch_news_sentiment` signature on the trait.** Match the existing call site: `async fn fetch(&self, symbol: &str, lookback_hours: u32) -> Result<Vec<NewsItem>, NewsError>`. Internally, IBKR provider converts `lookback_hours` to the `start_date_time` / `end_date_time` pair `req_historical_news` wants.
- **Per-article body fetch policy.** `req_historical_news` returns headlines + minimal metadata. Body comes from `req_news_article`. Default: do NOT fetch article bodies in v1 — `NewsItem.summary` is the headline plus any preview the historical call returns. Fetching bodies for every item would multiply IBKR news quota; revisit only if `NewsInterpreter` quality drops materially.
- **Cache row shape under IBKR.** The existing `news_cache` payload is a JSON-encoded `Vec<NewsItem>`. IBKR provider writes the same shape. Decide whether to add a `source: "alphavantage" | "ibkr"` field to each `NewsItem` for debug provenance. Default: yes — costs nothing, helps diagnose post-cutover weirdness.
- **`AlphaVantageNewsProvider` mock-fallback semantics.** AV news currently returns empty on missing API key (warns, doesn't error). Mirror this in the adapter or surface as `NewsError::Other`? Default: surface as `NewsError::Other` (Hard Invariant #5: no silent fallback on the migration path). UI handling is downstream of the trait.

## Exit criteria

- Every previously-passing cargo + vitest test still passes — this phase's behavior is byte-identical to today when `news_source = "alpha_vantage"`.
- New test: `IbkrNewsProvider::fetch("AAPL", 24)` against `MockIbkrClient` (loaded from Phase 6 fixtures) returns a non-empty `Vec<NewsItem>` whose populated fields cover at least: `time_published`, `title`, `url`, `source`, `summary`. `sentiment`-flavored fields are `None`.
- New test: provider against a mock that returns the TWS subscription-denied error (code 322) → `NewsError::NoSubscription { provider_code: ... }`.
- New test: provider against a mock that returns empty list → `Ok(vec![])`, not an error.
- New test: rate limiter test — N back-to-back `acquire()` + send pairs span ≥ the decided pacing window.
- New test: end-to-end through `news_cache` — provider call writes a row, `NewsInterpreter` reads it and produces a verdict, cache row gets `news_verdict_json` populated. Verifies `NewsInterpreter` survived the AV-fields-now-None change.
- Grep on news consumer paths shows zero direct `FinancialDataService::fetch_news_sentiment` references — only the AV adapter touches it.
- Settings file generated from a fresh launch contains `"news_source": "alpha_vantage"`.
- Pre-commit clean.

## Gotchas

- **`NewsInterpreter` reads cache rows.** If it reads any AV-only field (per-article sentiment scores), those reads need to handle `None`. The Phase 6 sentiment-loss audit identifies these; this phase fixes them. Without that fix, `news_source = "ibkr"` will silently produce empty verdicts.
- **`provider_codes` is required for `req_historical_news`.** Always pass the full subscribed list (cached from `req_news_providers` at startup). Don't hard-code provider codes.
- **TWS news pacing is per-provider, not global.** Hitting Reuters and Briefing in parallel is fine; hitting Reuters twice in 100ms is not. The rate limiter must serialize per-provider — a single global limiter under-utilizes the quota.
- **`time_published` is timezone-sensitive.** AV returns ET-tagged strings; IBKR returns Unix epoch (seconds, UTC). Normalize to a single shape in the parser; consumers downstream may sort or filter by time and a mismatch will silently break sort order.
- **Don't preemptively delete `FinancialDataService` here.** Phase 8 owns the deletion. Phase 7 ships even if Phase 8 hasn't started — that's the value of clean phasing.
- **`FundamentalsProvider` trait already established the pattern.** Don't re-debate trait shape (`async-trait`, `Send + Sync + 'static`, dyn-compat) — copy the pattern from `services/fundamentals_provider/mod.rs` so reviewers don't have to context-switch.
- **MCP `get_news` (or whichever news tool) gains nothing new.** Surveillance-only invariant — same tool, different backend. Don't sneak in extra fields under the migration banner.
