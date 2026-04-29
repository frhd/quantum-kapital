# Phase 14 — Intraday scheduler

## Goal

A long-running task that wakes every 5 minutes during RTH, fetches intraday bars for tickers in `InPlay` or `SetupActive`, re-runs detectors and decay-watcher (Phase 18), and emits any new `SetupDetected` / `SetupInvalidated` events.

## Depends on

- [x] Phase 13 — EOD scheduler / `StreamHandle` pattern in place.
- [x] Phase 12 — state machine + `active_in_play_symbols`.

## Out of scope

- Decay-watcher LLM call (Phase 18 owns prompt + integration; this phase calls a stub method that Phase 18 fills in).
- Per-minute resolution (5-min interval is the design; Phase 22 may make it tunable).

## Test plan (write tests FIRST)

`src-tauri/src/services/intraday_scheduler/tests.rs`.

- [x] `does_not_run_outside_rth` — clock at 18:00 ET → no-op.
- [x] `does_not_run_on_holiday`.
- [x] `runs_only_for_in_play_symbols` — watchlist: 5 watching, 2 in-play, 1 setup-active; tick during RTH invokes `runner.run_for` for the 3 in-play/setup-active only.
- [x] `runs_decay_watcher_for_active_setups` — for each `SetupActive` ticker with active setups, calls a stub `DecayWatcher::check(setup_id)`. (Stub returns `still_valid=true` until Phase 18.)
- [x] `decay_watcher_invalidation_flips_state` — when stub returns `still_valid=false, reason="..."`, scheduler calls `state_machine.mark_invalidated(setup_id, reason)`.
- [x] `tick_interval_is_5_minutes` — clock advances 4:30; no second run. Advances to 5:00; second run triggers.
- [x] `start_replaces_existing_handle` — same pattern as EOD.
- [x] `errors_in_one_symbol_dont_block_others` — `runner.run_for` errors for symbol A; symbols B/C still processed.

## Implementation tasks

- [x] Create `src-tauri/src/services/intraday_scheduler.rs`:
  ```rust
  pub struct IntradayScheduler {
      runner: Arc<TrackerRunner>,
      state_machine: Arc<TrackerStateMachine>,
      decay_watcher: Arc<DecayWatcherStub>, // replaced with real impl in Phase 18
      emitter: Arc<EventEmitter>,
      clock: Arc<dyn Clock>,
  }
  impl IntradayScheduler {
      pub async fn spawn(self: Arc<Self>) -> StreamHandle;
  }
  ```
  Inside `spawn`, `tokio::time::interval(Duration::from_secs(300))`. On tick:
  1. If shutdown → break.
  2. If not RTH → continue.
  3. `symbols = state_machine.active_in_play_symbols()`.
  4. For each symbol, `runner.run_for(symbol)`.
  5. For each `SetupActive` ticker, fetch active setups; call `decay_watcher.check(setup_id)`; on `still_valid=false`, invalidate.
  6. Errors collected and logged; do not abort the loop.
- [x] Create `src-tauri/src/services/decay_watcher.rs` with a `DecayWatcherStub { check } -> DecayDecision { still_valid: bool, reason: Option<String>, suggested_action: Option<String> }`. Trait or struct stub that Phase 18 replaces with a real Anthropic-backed implementation.
- [x] Add `intraday_handle` to `IbkrState`; `start_intraday_scheduler` / `stop_intraday_scheduler` methods.
- [x] Extend the `tracker_start_scheduler` / `tracker_stop_scheduler` commands from Phase 13 to start/stop both schedulers.
- [x] Settings: add `intraday_tick_interval_secs: u64` to `AppConfig` (default 300). Use in scheduler. (Reuses existing `config` infrastructure.)

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::intraday_scheduler` — green (10 tests).
- [x] Tests cover RTH gating, weekend / holiday no-ops, in-play-only iteration, decay-watcher dispatch, invalidation propagation, 5-minute cadence, and per-symbol error isolation. Live-RTH manual verification deferred until Phase 15's frontend listeners land (no UI yet to observe events from).
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/intraday_scheduler.rs` (+ tests submodule)
- `src-tauri/src/services/decay_watcher.rs` (stub; Phase 18 fills it)

**Modified:**
- `src-tauri/src/ibkr/state.rs` (intraday_handle + methods)
- `src-tauri/src/ibkr/commands/tracker.rs` (extend scheduler commands)
- `src-tauri/src/config/settings.rs` (`intraday_tick_interval_secs`)
- `src-tauri/src/services/mod.rs`

## Scratchpad

None.

## Done when

Intraday scheduler runs every 5 min during RTH, only for in-play tickers, invokes runner + (stub) decay-watcher, can be started/stopped, replaces handles on restart.
