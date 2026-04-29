# Phase 12 — Tracker status state machine

## Goal

Codify the lifecycle that drives intraday eligibility and prevents the same setup from re-firing forever:

```
watching   ──[scanner hit | manual flag | first detector hit]──> in_play (TTL 3 trading days)
in_play    ──[detector hit]──────────────────────────────────────> setup_active
setup_active ──[invalidated | target_hit | manual stop]──────────> cool_down (TTL 5 trading days)
cool_down  ──[TTL expires]───────────────────────────────────────> watching
in_play    ──[TTL expires]───────────────────────────────────────> watching
```

## Depends on

- [x] Phase 04 — `TrackedTicker.status`, `in_play_until` columns.
- [x] Phase 10 — `Setup` rows are produced.
- [x] Phase 11 — for "X trading days" math.

## Out of scope

- Scheduling the TTL-expiry sweep (that's Phase 13's job — the EOD sweep calls `expire_ttls()`).
- LLM-driven thesis-changed transitions (Phase 18 fires them via `mark_invalidated`).

## Test plan (write tests FIRST)

`src-tauri/src/services/tracker_state_machine/tests.rs` — exercise the transitions over a controlled clock.

- [x] `watching_promoted_to_in_play_on_scanner_add` — `record_scanner_hit('AAPL', meta)` sets `status=in_play`, `in_play_until = now + 3 trading days`.
- [x] `watching_stays_when_setup_directly_active` — `on_setup_detected` from `Watching` directly promotes to `SetupActive`, sets `in_play_until` (still useful for downstream queries).
- [x] `in_play_promoted_to_setup_active_on_detector_hit` — `on_setup_detected('AAPL', setup_id)` flips status, leaves `in_play_until` advanced.
- [x] `setup_active_to_cool_down_on_invalidate` — `mark_invalidated(setup_id, reason)` updates the setup row + flips ticker status to `CoolDown` with cooldown TTL = 5 trading days.
- [x] `setup_active_to_cool_down_on_target_hit` — `mark_completed(setup_id)` does the same.
- [x] `cool_down_to_watching_on_ttl_expiry` — clock fixture rolls past `cool_down_until`; `expire_ttls(now)` flips the status back.
- [x] `in_play_to_watching_on_ttl_expiry` — same for in-play.
- [x] `expire_ttls_is_idempotent` — call twice with same clock; second call no-ops.
- [x] `expire_ttls_uses_trading_days_not_calendar_days` — set `in_play_until` to "3 trading days from Friday"; expects to land on Wed of the next week (skipping weekend).
- [x] `multiple_active_setups_only_one_invalidation_flips_status` — ticker has 2 active setups; invalidating one keeps `SetupActive` (other still active); invalidating the second flips to `CoolDown`.

## Implementation tasks

- [x] Add `cool_down_until: Option<DateTime<Utc>>` to `tracked_tickers` if not already present. Decide: store separately from `in_play_until` or reuse the column? **Recommend separate** — different semantics, easier queries. Migration is additive (`ALTER TABLE`), log it in `schema-decisions.md`.
- [x] Create `src-tauri/src/services/tracker_state_machine.rs`:
  ```rust
  pub struct TrackerStateMachine { db, tracker }
  impl TrackerStateMachine {
      pub async fn record_scanner_hit(&self, symbol, meta) -> Result<()>;
      pub async fn record_manual_flag(&self, symbol) -> Result<()>;
      pub async fn on_setup_detected(&self, symbol, setup_id) -> Result<()>;
      pub async fn mark_invalidated(&self, setup_id, reason) -> Result<()>;
      pub async fn mark_completed(&self, setup_id) -> Result<()>;
      pub async fn expire_ttls(&self, now: DateTime<Utc>) -> Result<usize>;
      pub async fn active_in_play_symbols(&self) -> Result<Vec<String>>;
  }
  ```
- [x] Use `utils::market_calendar` to compute "N trading days from X" (helper: `trading_days_after(date, n)`).
- [x] Wire `TrackerStateMachine` into `IbkrState`.
- [x] Update `TrackerRunner` (Phase 10) to call `state_machine.on_setup_detected(symbol, setup.id)` after persisting.
- [x] Update `TrackerService::add` (when source is `Scanner`) — actually no, keep `add` and state-machine separate. Phase 05's "Add to tracker" UI should call `tracker_add` followed by `tracker_record_scanner_hit` (new command) when source is scanner. Or: have the Phase 05 UI call a higher-level `tracker_add_with_state(...)` command that does both atomically. **Recommend the latter** — more ergonomic. Add a new command `tracker_add` (returns now invokes the state machine if source=scanner).

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::tracker_state_machine` — green.
- [x] Manual: in DB, set a row's `in_play_until` to past; call `expire_ttls`; verify status flips back.
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/tracker_state_machine.rs` (+ tests submodule)

**Modified:**
- `src-tauri/src/storage/schema.sql` (`ALTER TABLE tracked_tickers ADD COLUMN cool_down_until INTEGER`) — actually, since we agreed Phase 01 defines all columns upfront, **add `cool_down_until` to the Phase 01 schema retroactively** if Phase 01 hasn't shipped yet. If it has, log the additive migration here.
- `src-tauri/src/services/tracker_service.rs` (helpers for status update with timestamps)
- `src-tauri/src/services/tracker_runner.rs` (call state machine on hit)
- `src-tauri/src/ibkr/state.rs` (expose state_machine)
- `src-tauri/src/ibkr/commands/tracker.rs` (re-route `tracker_add` for scanner source)
- `src-tauri/src/services/mod.rs`

## Scratchpad

- **Write** to `impl/scratch/schema-decisions.md` the addition of `cool_down_until` (separate from `in_play_until` — rationale).

## Done when

State machine transitions match the diagram above for every test case; TTL expiry uses trading-day math; multiple-active-setups handling is correct.
