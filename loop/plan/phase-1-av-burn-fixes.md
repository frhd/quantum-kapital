# Phase 1 — Stop the AV burn (frontend dedup + backend coalescing + rate limiter + stale-cache fallback)

> Part of [Alpha Vantage → IBKR Reuters](master.md). See index for invariants.

**Status:** todo

**Depends on:** none (foundation phase, independent of Phase 2)

**Goal:** Stop the duplicate-fetch storm and make the existing AV path resilient to rate-limit responses. After this phase, each UI projection screen render costs one set of three AV calls (not two), and concurrent agent-loop calls for the same symbol coalesce. Pays for itself even if the IBKR migration never ships.

## Files

- Touches: `src/features/analysis/hooks/useProjections.ts` — drop the redundant `getFundamentalData` call from `Promise.all`; consume bundled response.
- Touches: `src/shared/api/ibkr.ts` — update wrapper return type for `generateProjectionResults`.
- Touches: `src/shared/types/` (file owning `ProjectionResults`) — add the bundled `(FundamentalData, ProjectionResults)` response type.
- Touches: `src-tauri/src/ibkr/commands/analysis.rs` — `ibkr_generate_projection_results` (and `ibkr_generate_projections`) return fundamentals + projections together; remove the redundant inner `ibkr_get_fundamental_data` call.
- Touches: `src-tauri/src/services/financial_data_service/mod.rs` — in-flight coalescing in `fetch_fundamental_data`; rate-limiter wiring; stale-cache fallback in `fetch_av_function`.
- New: `src-tauri/src/middleware/alpha_vantage_rate_limit.rs` — 1 req/sec token bucket, mirrored from `historical_rate_limit.rs`.
- Touches: `src-tauri/src/middleware/mod.rs` — `pub mod alpha_vantage_rate_limit;`.
- Touches: `src-tauri/src/services/cache_service.rs` — `read_ignoring_ttl<T>` companion to existing `read<T>`.
- Touches: `src-tauri/src/lib.rs` — construct `AlphaVantageRateLimiter`; thread into `FinancialDataService::with_rate_limiter`.

## Reuse

- `middleware/historical_rate_limit.rs::HistoricalRateLimiter` — pattern (token bucket, async `acquire`).
- `services/financial_data_service/news.rs:280-286` — pattern for soft-skip + serve-stale-cache fallback.
- Existing `CacheService::read<T>` — keep intact; the new `read_ignoring_ttl<T>` is parallel, not a replacement.
- Existing `tokio::sync::Mutex` patterns elsewhere in `services/` for the per-symbol coalescing map.

## Decisions to make in this phase

- **Coalescing primitive:** per-symbol `Mutex<HashMap<String, Weak<Shared<...>>>>` vs. a single global `Mutex<HashMap>` with `OnceCell`-per-key vs. an external crate (`async-singleflight`). Decide before writing the test. Default: hand-rolled with `tokio::sync::broadcast` to avoid a new dep.
- **Bundled-response shape:** new struct `ProjectionResultsWithFundamentals { fundamentals, results }` vs. add `fundamentals` field to `ProjectionResults` directly. Default: new struct (keeps `ProjectionResults` clean for callers that only want the projection).

## Exit criteria

- One UI render of the analysis projection screen (StrictMode on, default dev) produces **one** `Fetching real fundamental data` log line and **three** downstream AV endpoint fetches. Verified with `RUST_LOG=info` while opening the screen for an uncached symbol.
- Vitest test on `useProjections`: a StrictMode render triggers exactly one `generateProjectionResults` invocation; zero `getFundamentalData` invocations.
- Cargo test on `FinancialDataService`: 10 concurrent `fetch_fundamental_data("AAPL")` calls produce exactly one set of three AV HTTP requests (verified via the test transport).
- Cargo test on `AlphaVantageRateLimiter`: 5 back-to-back `acquire().await` + send pairs span ≥4 seconds wall clock.
- Cargo test on `fetch_av_function` stale-cache fallback: cache primed with expired data, mock AV returns the `Information` rate-limit payload, the call returns the stale data and emits a `warn!`.
- `cargo test`, `pnpm test:run`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `pnpm lint`, `pnpm typecheck` all green. Pre-commit clean.

## Gotchas

- **React StrictMode is dev-only.** Production builds don't double-mount, so the absence of a duplicate fetch in dev is the bar. Don't write the test in a way that would pass without StrictMode.
- **Coalescing must release on error.** A failed in-flight fetch should not poison the slot for future callers. Either drop the slot on `Err`, or re-attempt on the next call; pick one and test it.
- **`tokio::try_join!` semantics.** All three AV endpoints fire in parallel; the rate limiter must serialize them. Without that, you've added a limiter that doesn't actually limit. The test that proves this fires three sequential requests, not one.
- **Stale-cache fallback can serve very stale data.** Add a max-age log warning (e.g., warn loudly if stale > 30 days). Don't silently serve year-old fundamentals.
- **Cache write on rate-limit.** Confirm the existing code does NOT write to cache when AV returns the `Information` field — that would poison the cache. The existing `fetch_av_function` checks the error before writing, so this should hold; verify with a test.
- **AV news path uses the same `FinancialDataService`.** The rate limiter must apply to news too (same vendor, same quota) — wire it in `news.rs` as well, not just the fundamentals path.
