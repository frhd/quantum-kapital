# Phase 02 — Historical bars service

## Goal

Add a working historical-bars fetch path (IBKR → service → SQLite cache → caller) so detectors in Phases 07–09 can ask for "last 200 daily bars of AAPL" without rate-limit grief or repeat IBKR calls.

## Depends on

- [x] Phase 01 — `Db` is available.

## Out of scope

- Streaming bar updates (we only fetch on demand).
- Tick-level data.
- Adjusted vs raw price reconciliation — use IBKR's adjusted `TRADES` series.

## Test plan (write tests FIRST)

All in `src-tauri/src/services/historical_data_service/tests.rs` and a small integration test for the `IbkrClient` method gated behind `#[cfg(feature = "ibkr-live")]`.

- [x] `cache_hit_returns_cached_bars_without_calling_client` — `MockIbkrClient` wired to fail; service still returns bars previously inserted into `bars_cache`. Verifies cache-first reads.
- [x] `cache_miss_fetches_and_writes_through` — Mock returns 5 bars; service returns the 5; second call returns the same 5 with the mock now expecting zero calls.
- [x] `partial_cache_fetches_only_missing_range` — 100 daily bars cached for AAPL ending at day T-50; request for 200 bars ending today triggers an IBKR call for the gap only.
- [x] `daily_bars_cached_indefinitely` — `should_refetch(symbol, BarSize::Day1, time_range)` returns false when bars exist regardless of age.
- [x] `intraday_bars_cached_only_for_today` — same call after day rolls over invalidates the cache for `BarSize::Min5` from a prior trading day.
- [x] `rate_limiter_invoked_per_request` — mock `RateLimiter` records one `acquire()` per IBKR fetch (not per cache hit).
- [x] `service_dedups_in_flight_requests_for_same_key` — two concurrent calls for the same `(symbol, bar_size, range)` collapse into one IBKR request. (Use `tokio::sync::Mutex<HashMap<...>>` for in-flight tracking.)
- [x] `bars_round_trip_through_sqlite_preserve_floats_and_volume` — write 1000 bars, read back; assert exact equality.

## Implementation tasks

- [x] Add `historical_data` rate-limit policy: 6 req/min for historical (separate from the 50 req/s `RateLimiter`). Either parameterize the existing `RateLimiter` or add a second instance dedicated to historical calls. _(Added `HistoricalRateLimiter` in `middleware/historical_rate_limit.rs`; constructed as `Arc::new(HistoricalRateLimiter::new(6))` in `lib.rs`.)_
- [x] In `src-tauri/src/ibkr/client.rs`, add:
  ```rust
  pub async fn historical_data(&self, req: HistoricalDataRequest)
      -> Result<Vec<HistoricalBar>, IbkrError>
  ```
  Wraps `ibapi::Client::historical_data()`; converts ibapi's bar struct → `HistoricalBar`. Use `tokio::task::spawn_blocking` since ibapi sync API is blocking. _(Implemented as `IbkrClient::get_historical_data` at `src-tauri/src/ibkr/client.rs:542-635`.)_
- [x] Mirror in `MockIbkrClient` (`src-tauri/src/ibkr/mocks.rs`) so tests can configure responses.
- [x] Create `src-tauri/src/services/historical_data_service.rs`:
  - `HistoricalDataService { db: Arc<Db>, client: Arc<dyn IbkrApi>, hist_rate_limit: Arc<RateLimiter> }`.
  - Public: `async fn fetch_bars(&self, symbol: &str, bar_size: BarSize, lookback: Lookback) -> Result<Vec<HistoricalBar>>`.
  - Internal: `read_cache`, `write_cache`, `compute_missing_ranges`, `inflight_dedup`. _(Used `HistoricalDataFetcher` trait + `IbkrClient` blanket impl rather than referencing the full `IbkrApi` trait directly.)_
- [x] `Lookback` enum: `Days(u32)` for daily and `TradingDay(NaiveDate)` for intraday.
- [x] Add `services` re-export in `src-tauri/src/services/mod.rs`.
- [x] Add Tauri command `tracker_fetch_bars(symbol, bar_size, lookback_days) -> Vec<HistoricalBar>` in a new `src-tauri/src/ibkr/commands/tracker.rs` file (this command is part of the tracker surface area; full set lands in Phase 04).
- [x] Register the command in `lib.rs`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::historical_data_service::` — all green (9 tests).
- [ ] Manual with TWS connected: invoke `tracker_fetch_bars('AAPL', '1day', 200)` from devtools, verify 200 bars returned. _(Deferred — requires live TWS session; covered by integration test scaffolding.)_
- [ ] Re-invoke; verify `bars_cache` row count grew on first call but not the second; IBKR data callbacks should fire once. _(Deferred — same.)_
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/historical_data_service.rs` (+ submodule for tests)
- `src-tauri/src/ibkr/commands/tracker.rs` (file scaffold; only one command for now)

**Modified:**
- `src-tauri/src/ibkr/client.rs` (+ `historical_data` method)
- `src-tauri/src/ibkr/mocks.rs`
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/ibkr/commands/mod.rs`
- `src-tauri/src/lib.rs` (register `tracker_fetch_bars`)

## Scratchpad

- **Read** `impl/scratch/schema-decisions.md` for `bars_cache` indexing rationale.
- **Write to** `impl/scratch/schema-decisions.md` if you discover the cache benefits from an additional index (e.g., `(symbol, bar_size, bar_time DESC)`).

## Done when

`tracker_fetch_bars` returns daily bars from IBKR, second call hits cache, intraday cache invalidates at session rollover, all tests green.
