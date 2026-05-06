# Phase 11 — Tilt circuit breaker

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** done (commit bfd1d2d, 2026-05-06)

**Depends on:** 1, 4

**Goal:** Account-level cool_down. After a -3R day or two consecutive losing closed trades, freeze new setup placement for the rest of the session. Behavioral tags currently detect tilt patterns *after* the fact; this phase prevents the third revenge trade by hard-pausing entries. Manual override exists, is logged, and counts toward trader-profile.

## Files

- New: `src-tauri/src/services/tilt_guard/mod.rs` — `TiltGuard::is_paused() -> Option<PauseReason>`.
- New: `src-tauri/src/services/tilt_guard/triggers.rs` — Pure functions over today's executions + R-stream; testable.
- New: `src-tauri/src/services/tilt_guard/state.rs` — Persistent pause state with auto-reset at next session open.
- Touches: `src-tauri/src/services/risk_engine/sizing.rs` — `RiskEngine::size` consults `TiltGuard::is_paused`; if paused, returns `Sizing::Skipped { reason: TiltPaused, until }`.
- Touches: `src-tauri/src/services/order_ticket/` — `OrderTicket::with_brackets` rejects if `TiltGuard::is_paused` AND no override. Override path requires `override_reason` from UI.
- Touches: `src-tauri/src/storage/migrations/` — `tilt_episodes` table: `id`, `triggered_at`, `trigger_kind`, `cumulative_r`, `consecutive_losses`, `released_at`, `release_kind` (auto / manual_override / session_end).
- New: `src-tauri/src/ibkr/commands/tilt_guard.rs` — `tilt_guard_status`, `tilt_guard_override`, `tilt_guard_history`.
- New: `src/features/portfolio/components/TiltBanner.tsx` — Persistent red banner when paused; shows trigger reason and reset time. Dismiss button requires `override_reason`.
- New: `src/features/trade-review/components/TiltHistoryCard.tsx` — Past tilt episodes for trader-profile rollup.
- Touches: `src/features/tracker/components/TakeSetupModal.tsx` — Modal disabled with "Tilt-paused — override required" footer when active.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `tilt_guard_status` | Current pause state + trigger details. |
| `tilt_guard_override` | Lift the pause for the rest of session; requires reason. |
| `tilt_guard_history` | Past tilt episodes. |

## Reuse

- P1 sizing path is the chokepoint — already consults a `Sizing::Skipped` enum with a `TiltPaused` variant reserved.
- P3 `OrderTicket` is the order chokepoint — already routes through a single function.
- P4 R-tracking gives the cumulative day R for trigger evaluation.
- Existing per-ticker `cool_down` state machine in `tracker_state_machine` — pattern reused, but tilt is account-wide, not ticker-wide.
- `EventEmitter` for `TiltActivated` and `TiltReleased` events.

## Decisions to make in this phase

- **Trigger thresholds.** Master committed: `-3R cumulative day` OR `2 consecutive -1R closed trades`. **Decision: as committed.** "Closed trade" means the bracket fully resolved (stopped or last-target hit / time-stopped); intra-trade drawdown doesn't count.
- **Trigger evaluation cadence.** **Decision: evaluate on every `BracketStatusChanged` event when status reaches a terminal state.** Not on tick / not on intraday MTM swing.
- **Reset condition.** **Decision: auto-reset at next US/Eastern session open.** Manual override before then is allowed once per session; second override in same session requires a 5-minute cooldown click.
- **Override audit.** **Decision: every override writes to `tilt_episodes.release_kind = manual_override` with the trader's reason text. Trader-profile counts overrides by month.**
- **Should tilt close existing positions?** **Decision: NO.** Tilt prevents *new* setups; existing brackets continue. Risk-management within a tilt-day is the bracket's job.
- **Two-day tilt.** A losing day followed by a worse opening hour. **Decision: a tilt episode that was overridden on day N raises day N+1's threshold by 1R for that day only (so -2R triggers tilt that day).** This is a soft penalty for ignoring tilt.
- **Coupling with conviction-scaled sizing (master open risk).** Calibrated A-conviction can drive larger sizing AND larger R losses. **Decision: tilt thresholds are R-based, not dollar-based, so calibration coupling is bounded by the R-cap.** Document.

## Exit criteria

- `cargo test tilt_guard::` passes: -3R day triggers, 2-consecutive-losses triggers, auto-reset at session open, override flow, second-override cooldown, day-N+1 stricter threshold after override.
- Integration test: walk a fixture session through 2 losing closed trades → `tilt_guard_status` returns Paused → next setup attempt fails with `TiltPaused` skip reason → override succeeds with reason → next setup attempt sizing returns normally.
- Frontend: TiltBanner appears within 1 second of tilt activation; dismiss requires reason text; persists across page reload.
- Migration clean; `tilt_episodes` table populated for live tilt events.
- Trader-profile rollup query includes tilt-episode count per month.

## Gotchas

- **Intraday partial wins / losses.** Mid-trade MTM swings don't count — only closed terminal R. Otherwise momentary drawdown trips tilt during a normal-volatility trade.
- **Stale snapshots.** P1 equity snapshot is T-1; during tilt-recovery sizing rules kick back in at next session open with refreshed snapshot. Make sure snapshot refresh and tilt reset happen in the right order (snapshot first).
- **Session-open detection.** "Next session" depends on calendar — Friday tilt resets on Monday. Calendar utility handles this; test holidays.
- **Manual override abuse.** Override exists for a real reason (the system can be wrong). But systematic override is a tilt symptom in itself. Trader-profile must surface "overrides per month" prominently; consider a soft warning if > 3/month.
- **Order placement during tilt via direct TWS (out of band).** The system can't stop the trader from typing into TWS. Out-of-band fills (P2) get attributed; at EOD, P4 grade reflects the tilt-day damage. The point of the gate is in-app friction, not external blocker.
- **Test fixtures must include "winning recovery day" cases.** A day where trader takes a -1R loss, then a +2R win, then -1R: that's 2 losses but not consecutive. The trigger logic must reset consecutive counter on a winner.
- **No order placement from tilt service.** Tilt-guard is a gate, not an actor. It must NEVER place close-out orders even when triggered. Surveillance invariant intact.
- **Coupling with portfolio-risk gate.** A setup might be blocked by both gates simultaneously. Surface both reasons; trader can override individually but each override is logged separately.
