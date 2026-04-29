# Phase 13 — EOD scheduler

## Goal

A long-running task that wakes daily at 16:05 ET, runs `TrackerRunner::run_all`, expires TTLs, and emits a (Phase 20) MorningPack. Started/stopped via Tauri commands like the existing scanner / daily-P&L streams.

## Depends on

- [x] Phase 10 — `tracker_run_now` machinery exists.
- [x] Phase 11 — market calendar.
- [x] Phase 12 — state machine (`expire_ttls`).

## Out of scope

- Intraday scheduling (Phase 14).
- LLM thesis generation — this phase only triggers detection; thesis writing is Phase 17 hooked in via the `SetupDetected` event.

## Test plan (write tests FIRST)

`src-tauri/src/services/eod_scheduler/tests.rs` — most tests inject a clock to avoid waiting.

- [x] `does_not_run_outside_eod_window` — clock at 10:00 ET → loop tick is a no-op.
- [x] `runs_at_1605_et_on_weekday` — clock at 16:05 ET Tuesday → invokes `runner.run_all`, then `state_machine.expire_ttls`.
- [x] `does_not_run_on_weekend` — Sat 16:05 ET → no run.
- [x] `does_not_run_on_holiday` — observed July 4 16:05 ET → no run.
- [x] `dedup_runs_within_same_day` — two ticks within 16:05–16:09 → only runs once (track `last_eod_run_date`).
- [x] `start_replaces_existing_handle` — second `start_eod_scheduler` stops the first (mirrors `state.start_scanner` pattern in `state.rs:92-110`).
- [x] `stop_drops_handle` — `stop_eod_scheduler` then `eod_handle.read().is_none()`.

## Implementation tasks

- [x] Create `src-tauri/src/services/eod_scheduler.rs`:
  ```rust
  pub struct EodScheduler { runner, state_machine, emitter, clock }
  impl EodScheduler {
      pub async fn spawn(self: Arc<Self>) -> StreamHandle;
  }
  ```
  Inside `spawn`, create an `Arc<AtomicBool>` shutdown flag, `JoinHandle` from `tokio::spawn` with a `tokio::time::interval(60s)` loop. On each tick:
  1. If shutdown flag set → break.
  2. Read clock; check `is_rth_open`/`is_holiday`/weekday.
  3. If now is between 16:05 and 16:09 ET on a trading day AND `last_eod_run_date != today_et`:
     - Run `runner.run_all().await`.
     - Run `state_machine.expire_ttls(now).await`.
     - Emit `AppEvent::MorningPackReady { date }` (real ranker payload added in Phase 20; for now emit an empty marker).
     - Update `last_eod_run_date`.
- [x] Add `eod_handle: Arc<RwLock<Option<StreamHandle>>>` to `IbkrState` (mirroring `scanner_handle`).
- [x] Add `IbkrState::start_eod_scheduler()` and `stop_eod_scheduler()` mirroring `start_scanner` / `stop_scanner` (state.rs:92-110).
- [x] Add Tauri commands `tracker_start_scheduler` / `tracker_stop_scheduler` (single command pair starts both EOD and intraday after Phase 14 lands; for this phase only EOD is wired).
- [x] Register in `lib.rs`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::eod_scheduler` — green.
- [x] Manual: temporarily drop the EOD-window check to "any minute past :05" and observe a run within a minute. Restore the real check before commit.
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/eod_scheduler.rs` (+ tests submodule)

**Modified:**
- `src-tauri/src/ibkr/state.rs` (eod_handle + start/stop methods)
- `src-tauri/src/ibkr/commands/tracker.rs` (start/stop scheduler commands)
- `src-tauri/src/lib.rs` (register commands; auto-start scheduler in setup if user opts in via settings — default off)
- `src-tauri/src/events/emitter.rs` (`MorningPackReady` event variant — payload may be empty for now)

## Scratchpad

None.

## Done when

Scheduler runs once per trading day at 16:05 ET, no-ops on weekends/holidays, can be started and stopped from the frontend, replaces an existing handle on restart.
