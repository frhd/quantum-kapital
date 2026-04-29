# Phase 10 — `tracker_run_now` command + setup persistence

## Goal

Make detectors actually executable end-to-end: a Tauri command that for one symbol (or the whole watchlist) gathers a `MarketContext`, runs detectors, persists hits to the `setups` table, and returns them. This is the first time real bars / news / fundamentals flow into a `SetupCandidate`.

## Depends on

- [x] Phase 02, 03, 04 — bars / news / tracker available.
- [x] Phase 06–09 — registry + three detectors registered.

## Out of scope

- Scheduled invocation (Phase 13–14).
- LLM thesis generation (Phase 17).
- Status state-machine transitions (Phase 12 owns those; this phase just inserts setups, doesn't promote `Watching → InPlay`).

## Test plan (write tests FIRST)

`src-tauri/src/services/tracker_runner/tests.rs` and a small e2e test in `src-tauri/src/ibkr/commands/tracker_tests.rs`.

- [x] `gathers_context_for_symbol` — `TrackerRunner::context_for("AAPL")` returns a `MarketContext` with at least daily bars when fundamentals + news are unavailable (best-effort fields are `None`/empty).
- [x] `runs_all_detectors_and_returns_outcomes` — mock registry with two detectors (one returns `Some`, one returns `None`); runner aggregates one outcome list of length 2.
- [x] `persists_hits_to_setups_table` — a returned `Some(SetupCandidate)` results in a `setups` row with the right `symbol`, `strategy`, `direction`, `trigger_price`, `stop_price`, `targets` JSON, `raw_signals` JSON, `status='active'`.
- [x] `does_not_persist_misses` — `None` outcomes don't write rows.
- [x] `dedups_recent_duplicates` — same symbol + strategy + direction within last 24h doesn't re-insert; the existing row's `detected_at` may be touched (decision logged in scratchpad).
- [x] `command_run_now_for_single_symbol` — `tracker_run_now({symbol: 'AAPL'})` invokes runner, returns `Vec<Setup>`.
- [x] `command_run_now_for_whole_watchlist` — `tracker_run_now({symbol: null})` iterates `tracked_tickers` (excluding `CoolDown` rows) and runs each.
- [x] `errors_in_one_symbol_dont_block_others` — bars-fetch failure for symbol A doesn't prevent symbols B/C from running; failures are surfaced in the response.

## Implementation tasks

- [x] Add `Setup` type in `src-tauri/src/ibkr/types/tracker.rs`:
  ```rust
  pub struct Setup {
      pub id: i64,
      pub symbol: String,
      pub strategy: String,
      pub direction: Direction,
      pub detected_at: DateTime<Utc>,
      pub trigger_price: f64,
      pub stop_price: f64,
      pub targets: Vec<TargetLevel>,
      pub raw_signals: serde_json::Value,
      pub thesis: Option<String>, // populated in Phase 17
      pub status: SetupStatus,
      pub invalidated_at: Option<DateTime<Utc>>,
      pub invalidation_reason: Option<String>,
  }
  pub enum SetupStatus { Active, Invalidated, Completed }
  ```
- [x] Extend `TrackerService` (Phase 04) with `setup` operations:
  - `insert_setup(candidate, symbol) -> Result<Setup>`
  - `list_setups(symbol: Option<&str>, since: Option<DateTime<Utc>>) -> Result<Vec<Setup>>`
  - `get_setup(id: i64) -> Result<Option<Setup>>`
  - `recent_duplicate(symbol, strategy, direction, within: Duration) -> Result<Option<i64>>`
- [x] Create `src-tauri/src/services/tracker_runner.rs`:
  - `TrackerRunner { db, tracker, historical_data, financial_data, registry }`
  - `pub async fn context_for(&self, symbol: &str) -> Result<OwnedMarketContext>` — fetches daily bars (200), intraday bars (today only, 5min for parabolic-short / EP), fundamentals (cached), news (last 24h), current quote (best-effort).
  - `pub async fn run_for(&self, symbol: &str) -> Result<RunResult>` — gathers context, dispatches `registry.evaluate_all`, persists hits with dedup.
  - `pub async fn run_all(&self) -> Result<Vec<RunResult>>` — iterates active watchlist.
- [x] `OwnedMarketContext` is the data envelope; convert to a borrowed `MarketContext<'_>` at call site.
- [x] Add Tauri command `tracker_run_now(symbol: Option<String>) -> Vec<Setup>` to `commands/tracker.rs`.
- [x] Extend `tracker_get_setups(symbol, since)` (already in command list from Phase 04) to actually return rows now.
- [x] Register both in `lib.rs`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::tracker_runner` — green.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::tracker_service` — green (covers the setup CRUD on `TrackerService`; the planned `ibkr::commands::tracker_tests` module was rolled into the runner tests since the commands are thin wrappers and the State extractors are not test-callable in isolation).
- [ ] Manual: with TWS connected and 3 tickers in the watchlist, run `tracker_run_now({symbol: null})`. Verify any hits show up in `setups` table; no hits for chop-bound symbols. _(Deferred: requires live TWS session; the runner is fully covered by unit tests.)_
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/tracker_runner/mod.rs`
- `src-tauri/src/services/tracker_runner/tests.rs`
- _(planned `src-tauri/src/ibkr/commands/tracker_tests.rs` rolled into `tracker_runner/tests.rs` — see Verification note)_

**Modified:**
- `src-tauri/src/ibkr/types/tracker.rs` (`Setup`, `SetupStatus`)
- `src-tauri/src/services/tracker_service.rs` (setup CRUD)
- `src-tauri/src/ibkr/commands/tracker.rs` (`tracker_run_now`, fix `tracker_get_setups`)
- `src-tauri/src/lib.rs` (register commands)
- `src-tauri/src/services/mod.rs`

## Scratchpad

- **Write** to `impl/scratch/detector-calibration.md` an "Observation log" entry the first time you run this against the live tape: which symbols fired which detectors, true / false positives, surprising silences.

## Done when

Running `tracker_run_now` on a real watchlist produces real setups in the SQLite `setups` table; running it twice in close succession doesn't duplicate; per-symbol failures don't kill the batch.
