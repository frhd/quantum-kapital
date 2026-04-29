# Phase 02 — Historical bars service

## Goal

Add a working historical-bars fetch path (IBKR → service → SQLite cache → caller) so detectors in Phases 07–09 can ask for "last 200 daily bars of AAPL" without rate-limit grief or repeat IBKR calls.

## Depends on

- [ ] Phase 01 — `Db` is available.

## Out of scope

- Streaming bar updates (we only fetch on demand).
- Tick-level data.
- Adjusted vs raw price reconciliation — use IBKR's adjusted `TRADES` series.

## Test plan (write tests FIRST)

All in `src-tauri/src/services/historical_data_service/tests.rs` and a small integration test for the `IbkrClient` method gated behind `#[cfg(feature = "ibkr-live")]`.

- [ ] `cache_hit_returns_cached_bars_without_calling_client` — `MockIbkrClient` wired to fail; service still returns bars previously inserted into `bars_cache`. Verifies cache-first reads.
- [ ] `cache_miss_fetches_and_writes_through` — Mock returns 5 bars; service returns the 5; second call returns the same 5 with the mock now expecting zero calls.
- [ ] `partial_cache_fetches_only_missing_range` — 100 daily bars cached for AAPL ending at day T-50; request for 200 bars ending today triggers an IBKR call for the gap only.
- [ ] `daily_bars_cached_indefinitely` — `should_refetch(symbol, BarSize::Day1, time_range)` returns false when bars exist regardless of age.
- [ ] `intraday_bars_cached_only_for_today` — same call after day rolls over invalidates the cache for `BarSize::Min5` from a prior trading day.
- [ ] `rate_limiter_invoked_per_request` — mock `RateLimiter` records one `acquire()` per IBKR fetch (not per cache hit).
- [ ] `service_dedups_in_flight_requests_for_same_key` — two concurrent calls for the same `(symbol, bar_size, range)` collapse into one IBKR request. (Use `tokio::sync::Mutex<HashMap<...>>` for in-flight tracking.)
- [ ] `bars_round_trip_through_sqlite_preserve_floats_and_volume` — write 1000 bars, read back; assert exact equality.

## Implementation tasks

- [ ] Add `historical_data` rate-limit policy: 6 req/min for historical (separate from the 50 req/s `RateLimiter`). Either parameterize the existing `RateLimiter` or add a second instance dedicated to historical calls.
- [ ] In `src-tauri/src/ibkr/client.rs`, add:
  ```rust
  pub async fn historical_data(&self, req: HistoricalDataRequest)
      -> Result<Vec<HistoricalBar>, IbkrError>
  ```
  Wraps `ibapi::Client::historical_data()`; converts ibapi's bar struct → `HistoricalBar`. Use `tokio::task::spawn_blocking` since ibapi sync API is blocking.
- [ ] Mirror in `MockIbkrClient` (`src-tauri/src/ibkr/mocks.rs`) so tests can configure responses.
- [ ] Create `src-tauri/src/services/historical_data_service.rs`:
  - `HistoricalDataService { db: Arc<Db>, client: Arc<dyn IbkrApi>, hist_rate_limit: Arc<RateLimiter> }`.
  - Public: `async fn fetch_bars(&self, symbol: &str, bar_size: BarSize, lookback: Lookback) -> Result<Vec<HistoricalBar>>`.
  - Internal: `read_cache`, `write_cache`, `compute_missing_ranges`, `inflight_dedup`.
- [ ] `Lookback` enum: `Days(u32)` for daily and `TradingDay(NaiveDate)` for intraday.
- [ ] Add `services` re-export in `src-tauri/src/services/mod.rs`.
- [ ] Add Tauri command `tracker_fetch_bars(symbol, bar_size, lookback_days) -> Vec<HistoricalBar>` in a new `src-tauri/src/ibkr/commands/tracker.rs` file (this command is part of the tracker surface area; full set lands in Phase 04).
- [ ] Register the command in `lib.rs`.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml services::historical_data_service::` — all green.
- [ ] Manual with TWS connected: invoke `tracker_fetch_bars('AAPL', '1day', 200)` from devtools, verify 200 bars returned.
- [ ] Re-invoke; verify `bars_cache` row count grew on first call but not the second; IBKR data callbacks should fire once.
- [ ] `cargo clippy ...`, `cargo fmt --check`.

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
