# Phase 5 — Event blackouts (earnings + FOMC)

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** done (commit 1fc50e7, 2026-05-06)

**Depends on:** none (foundation phase)

**Goal:** Refuse to fire setups inside event windows that are asymmetric blow-up risk for the configured detectors. Earnings and FOMC are the two cleanest candidates: known dates, known volatility expansion, mechanical to gate. The detectors don't need to be aware of events — the gate sits between detector output and persistence.

## Files

- New: `src-tauri/src/services/event_calendar/mod.rs` — `EventCalendarService::is_blackout(symbol, at) -> Option<Blackout>`.
- New: `src-tauri/src/services/event_calendar/earnings.rs` — `EarningsCalendar` trait + Alpha-Vantage adapter; falls back to manual store; refreshes weekly.
- New: `src-tauri/src/services/event_calendar/fomc.rs` — `FomcCalendar` with hardcoded next-18-months FOMC dates loaded from a small JSON file (`src-tauri/data/fomc_dates.json`).
- New: `src-tauri/src/services/event_calendar/types.rs` — `Blackout { kind: Earnings | Fomc, window: (start, end), reason: String }`.
- Touches: `src-tauri/src/services/tracker_runner/mod.rs` — Before persisting `SetupCandidate`, call `event_calendar.is_blackout(symbol, now)`. If blackout, write `setups.skipped_reason` and emit `SetupSkipped { reason }` instead of `SetupDetected`.
- Touches: `src-tauri/src/strategies/candidate.rs` — Add `skipped_reason: Option<SkipReason>` field. `SkipReason::EarningsBlackout`, `::FomcBlackout`, `::ZeroR`, `::BelowMinRisk`, `::TiltPaused` (P11), etc.
- Touches: `src-tauri/src/storage/migrations/` — `setups.skipped_reason TEXT`, `setups.skip_window_json TEXT`. Backfill NULL.
- New: `src-tauri/data/fomc_dates.json` — FOMC meeting dates 2026-2027.
- Touches: `src-tauri/src/storage/migrations/` — `event_calendar_cache` table for earnings: `symbol`, `next_earnings_date`, `confidence`, `fetched_at`, `source`.
- New: `src-tauri/src/ibkr/commands/event_calendar.rs` — `event_calendar_lookup(symbol)`, `event_calendar_force_refresh()`.
- Touches: `src/features/tracker/components/SetupCard.tsx` — Show "skipped: earnings in 3 BD" badge when `skipped_reason` set.
- Touches: `src/features/tracker/components/SkippedSetupsPanel.tsx` (new) — List of skipped setups today with reason; lets trader override per-setup ("I want this anyway") with logged reason.
- New: `src/shared/api/eventCalendar.ts`.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `event_calendar_lookup` | Returns next earnings + days-to-FOMC for a symbol. |
| `event_calendar_force_refresh` | Refresh cache; useful before morning sweep. |
| `setup_override_blackout` | Trader override: take a blackout-skipped setup anyway, with required reason text. |

## Reuse

- Existing AV fundamentals adapter has earnings-date data. Reuse the existing rate-limited path — do not add a new AV client.
- Existing `manual_fundamentals_store` for operator-curated earnings dates (overrides AV when present).
- Existing market calendar (`utils/market_calendar/`) for trading-day arithmetic (5 BD before earnings).

## Decisions to make in this phase

- **Earnings window default.** Master committed: 5 BD pre + 1 BD post. **Decision: configurable per detector.** Breakout: full 5+1. Parabolic short: 10 BD pre (more sensitive — pre-earnings ramps are common). Episodic pivot: 0 (it's literally meant to trade gap on news, including earnings news).
- **FOMC window default.** Master committed: day-of-FOMC, 14:00 ET → close. **Decision: as committed; widening to T-1 close → T+1 09:35 if backtest in P6 shows wider window helps.** Static for now.
- **Earnings source priority.** Manual store > AV cache (fresh < 7 days) > AV fetch > skip-with-warning. **Decision: as listed.** AV fetch counts against AV quota (existing ledger).
- **Unknown-earnings behavior.** Symbol has no earnings date in any source. **Decision: configurable per detector — `skip_if_unknown: true` is safer for breakout; `false` for episodic-pivot.** Default: `true`.
- **AV path retirement audit.** Master removals committed: "audit AV fundamentals fallback." **This phase performs the audit.** If only consumer is event-calendar earnings, evaluate replacing with a paid earnings-date provider OR keeping AV scoped to earnings only.
  - Decision: **Keep AV for earnings**, retire AV for fundamentals (revenue/EPS) if unused after audit. Move the decision into `QUESTIONS.md` with the audit result.

## Exit criteria

- `cargo test event_calendar::` passes: earnings window edge cases (5 BD before is correctly counted excluding holidays), FOMC day-of detection, manual-override-wins-over-AV.
- Integration test: detector fires for AAPL when next earnings is in 4 BD; setup is created with `skipped_reason: EarningsBlackout`; UI shows skipped panel; override produces a non-skipped setup with `override_reason` recorded.
- Backtest replay (against fixtures from P6 — synthetic until then) shows: blackout-gated detectors skip earnings windows in historical bars.
- AV-fundamentals audit committed to `QUESTIONS.md` under Phase 5; if retirement decided, the AV-fundamentals removal migration is included in this phase's diff.
- Pre-commit clean; new files under size caps.

## Gotchas

- **Earnings dates are estimates until announced.** AV's "earnings date estimate" can be off by a week. Document confidence; use `confidence` field to widen the window when low.
- **FOMC dates can be moved.** Hardcoded JSON must be reviewable; show "stale fomc dataset (last entry < 90 days from now)" warning.
- **Trading-day arithmetic.** "5 BD before" must respect US holidays — already in the calendar utility, but a fence-post error is easy here. Test with Memorial Day / Thanksgiving cases.
- **Episodic-pivot crosses-purposes risk.** Setting `earnings_window: 0` for episodic-pivot means it explicitly trades earnings gaps. That's the design — but P6 backtest must report episodic-pivot's earnings-bar performance separately so we can tell if it's the source of edge or the source of pain.
- **Override audit.** Override reason is required free text. Don't accept empty string — frontend validates; backend enforces.
- **Pre-P5 setups have no `skipped_reason`.** All NULL. UI handles NULL as "not-skipped" for backward compatibility.
- **Calendar-aware schedulers (Hard Invariant 6) — don't double-skip.** If the morning scheduler is already calendar-gated, the event-blackout layer applies on top, not instead. Test the composition.
- **AV ledger budget.** Earnings refreshes for the entire watchlist will burn the AV daily quota fast. Refresh weekly not daily; on-demand only when watchlist add-time delta > 7d.
