# Phase 7 — Vol-adjusted exits + trailing/partial/time stops

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** done (commit e775409, 2026-05-06)

**Depends on:** 6

**Goal:** Replace fixed `2R/3R` targets with a configurable exit policy: ATR-relative targets, trailing stop on the runner, partial profit-takes, and time stops. Validated by Phase 6 backtest before cutover. Eliminates the largest source of "drifted" outcomes in the existing calibration ladder, which suggests exits — not entries — are where the edge is leaking.

## Files

- Touches: `src-tauri/src/strategies/candidate.rs` — Replace `target_2r: f64` and `target_3r: f64` fields with `targets: Vec<TargetSpec>`. `SkipReason` already exists from P5; reuse.
- New: `src-tauri/src/strategies/exits/mod.rs` — `ExitPolicy` trait + factory.
- New: `src-tauri/src/strategies/exits/static_2r_3r.rs` — Legacy policy, retained for shadow-mode comparison.
- New: `src-tauri/src/strategies/exits/atr_scaled.rs` — Default new policy: 1×ATR, 2×ATR, 4×ATR runner with chandelier ATR-trail.
- New: `src-tauri/src/strategies/exits/trailing.rs` — Chandelier exit, BE-move-on-1R, configurable.
- New: `src-tauri/src/strategies/exits/time_stop.rs` — Close at N bars elapsed if neither target nor stop hit.
- Touches: `src-tauri/src/services/order_ticket/` — `with_brackets` accepts `Vec<TargetSpec>` (already does after P3); now plus a `trail_spec` and `time_stop_spec`. Active brackets are revised intraday by a new `BracketReviser` service when trail conditions trigger.
- New: `src-tauri/src/services/bracket_reviser/mod.rs` — Polls active brackets every N seconds, computes new stop price, sends modify-order request. Surveillance-friendly: only modifies child stops on already-confirmed brackets, never places new parents.
- Touches: `src-tauri/src/storage/migrations/` — `setups.exit_policy_version TEXT`, `setups.targets_json TEXT`. `bracket_groups.trail_state_json TEXT`.
- Touches: `src-tauri/src/services/tracker_runner/` — When constructing a setup, pull exit policy by detector × regime (regime from P9 if available, else default).
- New: `src/features/tracker/components/ExitPlanCard.tsx` — Shows targets, trailing stop logic, time-stop bars elapsed.
- Touches: `src/features/tracker/components/TakeSetupModal.tsx` — Render the full exit plan; trader can override per-setup.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `exits_get_policy` | Current exit policy per detector + regime. |
| `exits_set_policy` | Override exit policy (admin). |
| `bracket_reviser_status` | Current state of all active brackets, trail levels, time-stop bars elapsed. |

## Reuse

- P3 `OrderTicket::with_brackets` for atomic placement.
- P6 backtester to validate the new policy against the static 2R/3R baseline.
- ATR indicator from `strategies/indicators.rs`.
- IBKR modify-order primitives in `ibkr/client/orders.rs`.

## Decisions to make in this phase

- **Default ATR multiples.** **Decision: 1× / 2× / 4× ATR(20) at trigger time, with 50% / 30% / 20% allocation.** Per-detector overrides allowed.
- **Trailing-stop trigger.** **Decision: activate trail after first target fills (≥ 1×ATR profit). Pre-trigger, stop is fixed.**
- **Trail formula.** **Decision: chandelier — `max(stop_so_far, max_high_since_entry - 3×ATR)` for longs.** Inverted for shorts. ATR refreshed daily.
- **BE move.** **Decision: at 1R profit (not 1×ATR — these can differ), move stop to entry. Independent of trailing.**
- **Time-stop horizon.** Per detector. **Decision: breakout = 10 trading days. Parabolic short = 3 trading days. Episodic pivot = 5 trading days.** All configurable.
- **Shadow-mode duration.** **Decision: 4 weeks of live shadow** — backtest-validated policy runs alongside legacy on every setup; `exit_policy_version` records which was attached. Cutover when OOS Sharpe(new) ≥ Sharpe(legacy) AND profit-factor(new) ≥ profit-factor(legacy). If neither passes, retire vol-adjusted, keep static. Document outcome in `QUESTIONS.md`.
- **Bracket-modify cadence.** **Decision: every 60 seconds during RTH; every 5 min outside RTH for time-stop only.** Avoids hammering IBKR rate limits.

## Exit criteria

- `cargo test strategies::exits::` passes: ATR-target arithmetic, BE-move logic, chandelier-trail edge cases (gaps, overnight, missing bars), time-stop bar-counting (skips weekends/holidays).
- Backtest in P6 with `atr_scaled` policy compared head-to-head with `static_2r_3r` over 18 months OOS — comparison report in `QUESTIONS.md`.
- Live shadow mode: `bracket_groups` has rows for both policies for every setup over the 4-week window; `BracketReviser` modifies stop child orders correctly without breaking OCA group.
- Frontend `ExitPlanCard` renders correctly for ATR-scaled and static policies; trader override flow works.
- Migration applies cleanly; pre-P7 setups read with `exit_policy_version = "v1_static"`.
- Cutover decision committed to `QUESTIONS.md` Phase 7 section.

## Gotchas

- **Modify-order race conditions.** A trail bump can race with a fill on the trail-target. Always check `bracket_groups.last_status` before sending modify; abort modify if status changed since poll.
- **Chandelier on gap-down opens.** If a long gaps down through trail, the modify won't fire (the stop is already triggered). That's correct — but log the case for review.
- **Time stop on a winner.** Time-stop closing a small winner that was about to run is the painful kind of false stop. The 4-week shadow validates this; if time-stops cost more than they save in OOS, retire.
- **ATR-multiple targets in backtest must use ATR at signal time, not at exit time.** Common bug: leaks future vol into past targets.
- **OCA group reduction on partial fill.** When target-1 fills 50%, the trail-target (originally on full qty) must reduce to remaining 50%. IBKR handles this natively for OCA, but verify via paper account.
- **Regime-aware exits.** P9 may want to widen trail in trending regimes. Phase 7 ships regime-agnostic; P9 layers on top via per-regime config. Don't bake regime detection into exits in this phase.
- **Pre-P7 brackets are static.** They keep their original 2R/3R targets and don't get retroactively trailed. New policy applies only to setups created after P7 ships.
- **Reverter test.** A "panic button" command that cancels the trail and reverts to a static stop+target must exist (`order_ticket_cancel_bracket` from P3 already covers full cancel; add `bracket_revert_to_static` for partial revert).
