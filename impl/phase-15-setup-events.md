# Phase 15 — Setup events plumbing + frontend listeners

## Goal

End-to-end event flow: when a setup is detected or invalidated, the frontend receives a typed event and updates the UI without a full re-fetch. Surfaces these events in the Watchlist row badge + a small toast.

## Depends on

- [ ] Phase 13 / 14 — schedulers fire setups.
- [ ] Phase 05 — Watchlist UI exists.
- [ ] Phase 12 — state machine emits status changes too.

## Out of scope

- Dedicated `AlertFeed` UI (Phase 21 builds the rolling list).
- LLM thesis content in events (Phase 17 fills `thesis` field).

## Test plan (write tests FIRST)

Backend:

- [ ] `setup_detected_event_emitted_on_runner_persist` — `TrackerRunner` after persisting a setup emits `AppEvent::SetupDetected` via `EventEmitter`. Test using a captured-events `EventEmitter` test double.
- [ ] `setup_invalidated_event_emitted_on_state_machine_transition` — calling `state_machine.mark_invalidated(...)` emits `AppEvent::SetupInvalidated`.
- [ ] `ticker_status_changed_event_on_promotion` — `state_machine.record_scanner_hit` emits `AppEvent::TickerStatusChanged { from: Watching, to: InPlay }`.
- [ ] `events_serialize_with_camelcase_keys` — JSON of `SetupDetected` payload has `setupId`, `symbol`, `strategy`, `direction`, `triggerPrice`, `stopPrice`, `targets`, `convictionSignal`, `detectedAt`. (Match the existing event-emitter convention.)

Frontend (manual E2E):

- [ ] Trigger a detection by manual `tracker_run_now` → toast "BREAKOUT detected on AAPL @ $X" appears; Watchlist row shows a pulsing setup badge.
- [ ] Trigger invalidation manually via dev command → row badge clears; toast "AAPL breakout invalidated: <reason>".
- [ ] Multiple events arrive in fast succession → no race conditions in the listener; no duplicate toasts.

## Implementation tasks

Backend:

- [ ] Extend `src-tauri/src/events/emitter.rs` — add variants:
  ```rust
  AppEvent::SetupDetected { setup: Setup, thesis: Option<String> }
  AppEvent::SetupInvalidated { setup_id: i64, symbol: String, reason: String }
  AppEvent::TickerStatusChanged { symbol, from: TrackerStatus, to: TrackerStatus }
  AppEvent::MorningPackReady { date: NaiveDate, ranked_count: usize } // payload extended in Phase 20
  ```
  And matching event names: `setup-detected`, `setup-invalidated`, `ticker-status-changed`, `morning-pack-ready`.
- [ ] In `TrackerRunner::run_for` (Phase 10), after persisting a setup row, emit `SetupDetected`.
- [ ] In `TrackerStateMachine::mark_invalidated`, emit `SetupInvalidated`.
- [ ] In `TrackerStateMachine` transitions that change status, emit `TickerStatusChanged`.

Frontend:

- [ ] Create `src/features/tracker/hooks/useTrackerEvents.ts`:
  - Subscribes to all four events using existing Tauri `listen` pattern (see `src/features/scanner/hooks/useScanner.ts:11-60` for reference).
  - Maintains an in-memory event log (capped at last 100 events) for Phase 21 to consume.
  - Returns `{ recentEvents, lastSetupDetected, lastInvalidated }`.
- [ ] Wire `useTrackerEvents` into `useWatchlist` — invalidate the watchlist row for the affected symbol on each event so badges update.
- [ ] Add a small `Toast` primitive in `src/shared/components/ui/toast.tsx` if not present (lightweight, ~30 lines, uses Tailwind for transitions). Tie into `useTrackerEvents`.
- [ ] Watchlist row gains a `<SetupBadge setup={lastSetup} />` element rendering strategy + direction + trigger.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml events::` — green.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml services::tracker_runner` updated to assert `SetupDetected` is emitted.
- [ ] Manual E2E checklist above.
- [ ] `cargo clippy ...`, `cargo fmt --check`, `pnpm build`.

## Files

**Created:**
- `src/features/tracker/hooks/useTrackerEvents.ts`
- `src/shared/components/ui/toast.tsx` (if not already)
- `src/features/tracker/components/SetupBadge.tsx`

**Modified:**
- `src-tauri/src/events/emitter.rs`
- `src-tauri/src/services/tracker_runner.rs`
- `src-tauri/src/services/tracker_state_machine.rs`
- `src/features/tracker/components/Watchlist.tsx`
- `src/features/tracker/hooks/useWatchlist.ts`

## Scratchpad

None.

## Done when

All four events fire end-to-end, frontend listeners update the Watchlist badge and toasts without manual reload, no duplicate emissions.
