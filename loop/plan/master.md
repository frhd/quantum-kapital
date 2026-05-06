# Quantum Kapital → Quant-Decisions-In-Code: Q3 2026

Today every quant decision (sizing, exits, blackouts, regime, attribution) lives in the trader's head — the detector pipeline is observation only and the human is the edge. End state: the system owns the quant decisions deterministically, the LLM owns narrative, the human owns parent-order send and override.

## Context

**Today.** A surveillance + journaling app. Pipeline: scanners → candidate universe → watchlist → detectors → LLM thesis → state machine → alerts. Post-trade: executions ingest → FIFO trade legs → tag-weighted grade → trader-profile rollup. The trader manually decides qty, manually places orders in TWS, manually exits. Behavioral tags catch sizing/exit errors *after* the fact.

**End state.** The same surveillance pipeline, plus a deterministic risk engine that sizes every setup, attaches bracket exits at activation, attributes realized R per detector, and refuses to fire setups that fail event-blackout, portfolio-concentration, regime, or tilt gates. A backtester replays `bars_cache` through the live `DetectorRegistry` so detector parameters and the gate set are evidence-driven, not opinion-driven. The LLM keeps writing prose theses; it never sizes, never exits, never overrides a gate.

**Inversion to name explicitly.** "Setup detected → trader trades" becomes "setup detected → sized + gated + bracketed → trader confirms-and-sends, or override-with-reason." The system fills the gap between detection and order. The human stays in the loop for *every* parent order.

## End-state architecture

| Subsystem | Role | Owner phase |
|---|---|---|
| `services/risk_engine/` | Position sizing, conviction scaling, dollar-risk computation, equity snapshot policy | P1 |
| `services/tca/` | Setup↔execution linkage, arrival/fill slippage capture, per-strategy attribution | P2 |
| `ibkr/client/orders.rs` (extended) | Bracket-order submission (parent + stop + targets) under explicit per-setup human confirmation | P3 |
| `services/trade_reviews/` (rewrite) | R-adjusted scoring, equity curve, Sharpe/Sortino/Calmar/PF/expectancy, per-strategy attribution | P4 |
| `services/event_calendar/` | Earnings + FOMC blackouts as a detector gate | P5 |
| `services/backtester/` | Bar-replay harness, fill simulation with calibrated slippage, walk-forward splits | P6 |
| `strategies/exits/` | Vol-adjusted targets, trailing stops, partial scales, time stops | P7 |
| `services/portfolio_risk/` | Open-position dollar-risk, sector/factor concentration, pre-trade gate | P8 |
| `services/regime/` | Regime classifier (VIX/SPY/breadth/ADX); detectors declare preferred regimes | P9 |
| `services/param_refit/` | Walk-forward monthly OOS sweep; param-vintage recorded with each setup | P10 |
| `services/tilt_guard/` | Account-level circuit breaker (-3R day, consecutive losses, tag tripwire) | P11 |
| LLM (existing) | Narrative thesis, news polarity, ranking — never sizing, never exits, never overrides gates | unchanged |
| Human | Parent order send, gate override (logged), strategic decisions | unchanged |

## Hard invariants

These bind every phase. Violating the letter of the rules is violating the spirit.

1. **Surveillance-plus-confirmed-execution.** No scheduler, detector, agent, or LLM ever places a parent order. Bracket children may be attached *only* to a parent the human has just confirmed in our UI for that specific setup. Programmatic order placement requires a human-initiated confirmation event in the same session.
2. **All LLM calls go through `LlmService`.** Daily USD budget enforced server-side. Even narrative generation in the backtester (if any) routes through the ledger. Never bypass for tests; use the trait seam.
3. **Mock-friendly trait seams.** Every IBKR-touching service implements a trait used by tests against `MockIbkrClient` (or a phase-specific mock derived from it). No live IBKR in tests.
4. **Audit on writes.** Sizing decisions, bracket placements, blackout skips, regime skips, gate overrides, backtest runs, and param refits all persist with a `reason` field and a UTC timestamp. Decisions that flow back to the trader as warnings or skips must be queryable post-hoc.
5. **No backwards-compat shims for retired logic.** When a formula or threshold is replaced (e.g., the `net_pnl/100` grade in P4, the static `2R/3R` in P7), older rows stay immutable with a `prompt_version` / `formula_version` / `param_vintage` tag; no shim translates between old and new at read time.
6. **Calendar-aware schedulers.** Every intraday scheduler consults RTH + US-holiday calendars (existing in `utils/market_calendar/`). Phase 5 extends this with earnings + FOMC. Backtests honor the same calendar.
7. **File-size caps respected.** Soft 300/200, hard 500/350. New services that grow past hard cap split before merging; no `// allow-large-file` justifier in this program.
8. **Pre-commit hooks unmodified.** `cargo fmt --check`, `clippy -D warnings`, prettier, eslint. Never `--no-verify`. Clippy regressions fixed at root.
9. **CI-grep invariants.** Two greps run in CI:
   - `net_pnl\s*/\s*100` must not appear in non-test grading code after P4.
   - Any file under `services/` or `strategies/` that calls `place_order` directly (not through `OrderTicket::with_brackets`) fails CI after P3.

## Defaults committed

Locked once so phases don't re-debate. Overridable per-phase with a written justification.

| Default | Value | Rationale |
|---|---|---|
| Risk per trade (A conviction) | 0.50% of account equity | Half-Kelly-ish for retail; survives a 20-loss streak with 10% DD |
| Risk per trade (B / C) | 0.33% / 0.16% | Conviction-monotone; capped at 1.0× until P4 calibration justifies higher |
| Equity snapshot for sizing | T-1 close NLV | Avoids intraday MTM whiplash; recomputed at next market open |
| Backtest fill model (pre-P2) | Next-bar-open + 8 bps slippage | Replaced by P2-calibrated per-strategy distribution |
| Earnings blackout window | 5 BD pre + 1 BD post | Configurable per detector; default applies to all 3 existing |
| FOMC blackout | Day-of FOMC, 14:00 ET → close | Avoids the 2pm vol expansion |
| Tilt threshold | -3R cumulative day OR 2 consecutive -1R closed trades | Resets at next session open |
| Bracket structure | 50% at 1R + 30% at 2R + 20% runner with ATR-trail | Replaces hardcoded 2R/3R; validated by P6 backtest |
| Strategy attribution granularity | By detector class (breakout, parabolic_short, episodic_pivot) | Refinable to sub-strategy after P10 |
| LLM model selection | Existing: Sonnet 4.6 for thesis/ranking; Haiku 4.5 for decay-watcher | Unchanged by this program |

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1 | [phase-1-risk-engine.md](phase-1-risk-engine.md) | none (foundation) | done (commit 7d9251b, 2026-05-05) |
| 2 | [phase-2-tca-linkage.md](phase-2-tca-linkage.md) | none (foundation) | done (commit 1f9036b, 2026-05-05) |
| 3 | [phase-3-bracket-on-activation.md](phase-3-bracket-on-activation.md) | 1, 2 | done (commit b3da8de, 2026-05-06) |
| 4 | [phase-4-r-grading.md](phase-4-r-grading.md) | 1, 2 | done (commit 47eb50f, 2026-05-06) |
| 5 | [phase-5-event-blackouts.md](phase-5-event-blackouts.md) | none (foundation) | done (commit 1fc50e7, 2026-05-06) |
| 6 | [phase-6-backtester.md](phase-6-backtester.md) | 2 | done (commit 0f8d86e, 2026-05-06) |
| 7 | [phase-7-vol-adjusted-exits.md](phase-7-vol-adjusted-exits.md) | 6 | done (commit e775409, 2026-05-06) |
| 8 | [phase-8-portfolio-risk.md](phase-8-portfolio-risk.md) | 1 | done (commit fa3e570, 2026-05-06) |
| 9 | [phase-9-regime-gating.md](phase-9-regime-gating.md) | 6 | in-progress (started 2026-05-06) |
| 10 | [phase-10-walk-forward-refit.md](phase-10-walk-forward-refit.md) | 6 | todo |
| 11 | [phase-11-tilt-circuit-breaker.md](phase-11-tilt-circuit-breaker.md) | 1, 4 | todo |
| 12 | [phase-12-options-and-mtf.md](phase-12-options-and-mtf.md) | 6, 7, 9 | punted |

> When starting/exiting a phase, update **both** this row's `Status` AND the phase file's `**Status:**` header. Don't start a phase whose dependencies aren't `done`.

## Critical files

Cross-cutting references; phases link here instead of duplicating paths.

| Concern | Path |
|---|---|
| Service composition (where new services are wired) | `src-tauri/src/lib.rs::run` |
| IBKR shared root | `src-tauri/src/ibkr/state.rs` (`IbkrState`) |
| Detector trait + registry | `src-tauri/src/strategies/{trait_def.rs, registry.rs}` |
| Setup candidate (sizing target) | `src-tauri/src/strategies/candidate.rs` |
| Order placement (bracket extension point) | `src-tauri/src/ibkr/client/orders.rs` |
| Executions ingest (TCA hook) | `src-tauri/src/services/executions/` |
| FIFO leg matcher (per-strategy rollup hook) | `src-tauri/src/services/trade_legs/` |
| Trade-review grading (P4 rewrite target) | `src-tauri/src/services/trade_reviews/` |
| LLM budget ledger (must remain on path) | `src-tauri/src/services/llm_service/` |
| MCP read-only seam | `src-tauri/src/mcp/ibkr_seam.rs` |
| SQLite schema + migrations | `src-tauri/src/storage/{schema.sql, migrations/}` |
| Frontend setup card | `src/features/tracker/` |
| Frontend trade-review surface | `src/features/trade-review/` |
| Frontend portfolio (P8 surface) | `src/features/portfolio/` |
| RTH/holiday calendar | `src-tauri/src/utils/market_calendar/` |
| Test mock seam | `src-tauri/src/ibkr/mocks.rs` |

## Sequencing + cadence

Twelve calendar weeks. Phases 1, 2, 5 are independent and start in parallel.

| Week | In flight |
|---|---|
| 1 | P1 risk-engine, P2 TCA-linkage, P5 blackouts (parallel) |
| 2 | P1, P2, P5 finish; P3 brackets begins |
| 3 | P3 finishes; P4 R-grading + P8 portfolio-risk in parallel |
| 4 | P4, P8 finish; P6 backtester begins |
| 5 | P6 continues (largest single phase) |
| 6 | P6 finishes; P7 vol-adjusted exits begins |
| 7 | P7 finishes (4-week shadow start) |
| 8 | P9 regime gating |
| 9 | P10 walk-forward refit |
| 10 | P11 tilt circuit breaker |
| 11 | P7 shadow ends → cutover; OOS audit of P9/P10 results |
| 12 | Slack: end-to-end audit, doc, P12 scoping if pulled in |

P12 (options + MTF confluence) is **punted** — schedule when (a) backtester evidence shows directional edge on episodic-pivot is being leaked to IV crush, AND (b) IBKR data tier supports options chain pulls without breaking the rate budget.

## Cross-phase verification

Gates that span phases:

- **Tracer-bullet (after P4).** End-to-end: setup detected at 09:31 → sized by risk-engine → human confirms in UI → bracket placed → fill recorded with arrival slippage → realized R written to executions → grade reflects R, not net_pnl/100 → attributed to originating detector. This walk must work in `cargo test` against `MockIbkrClient` and live against IBKR paper.
- **CI-grep invariants (continuous from P3 + P4).** See Hard Invariant 9.
- **Shadow mode (P7).** Vol-adjusted exits run in shadow alongside legacy fixed targets for 4 weeks. Cut over to vol-adjusted only when OOS Sharpe ≥ static AND profit-factor ≥ static. If neither passes, retire vol-adjusted, document why in `QUESTIONS.md`, keep static.
- **Backtest currency invariant (continuous from P10).** Every detector live in production must have an OOS backtest entry within last 30 days. Missing → CI fails the nightly check.
- **Sizing-uses-equity-snapshot invariant (P1 + P11).** Tilt-paused accounts cannot have new sizing computed. P1 must surface paused-state from P11; P11 must not be loadable as a no-op from any P1 path.
- **Override audit (continuous).** Every gate override (blackout, concentration, regime, tilt) writes to `gate_overrides` table with `setup_id`, `gate_kind`, `reason`, `actor`, `at`. Reviewable in trader-profile.

## Open risks

| Risk | Owner phase |
|---|---|
| Backtest reveals one or more existing detectors are unprofitable post-realistic-slippage | P6 (retire-or-fix decision; executed by P7 if exit-policy fix; flagged to user if structural retirement) |
| Bracket orders interact unexpectedly with IBKR partial fills + cancel-on-fill behavior | P3 (must test against paper account before live; document IBKR-side OCO semantics) |
| Account equity changes intraday (drawdown, deposits) — does sizing recompute? | P1 (commits T-1 NLV snapshot; intraday recompute deferred to P11 tilt) |
| Regime gating reduces trade frequency below statistical significance | P9 (define minimum monthly N per detector; if violated, re-widen regime envelope OR retire) |
| Walk-forward refit overfits to recent regime | P10 (rolling 12-month window, min 200 trades, refuse refit if N too low) |
| PnL/100 grade replacement breaks UI components reading old field | P4 (new column + version flag; UI reads `score_v2`, falls through to `score` for pre-P4 rows) |
| Earnings/FOMC calendar source flakiness (AV rate-limited) | P5 (graceful degradation: skip-if-unknown configurable; FOMC dates hardcoded for 18 months out) |
| LLM thesis prose may not contribute measurable trade-outcome lift | P6 (A/B in backtest: outcomes for setups with thesis vs without; if no lift, demote thesis to optional, **do not** auto-disable — narrative has discipline value beyond outcome lift) |
| Sentiment-surge candidate source may underperform IBKR scanner candidates | P6 (compare detector-fire rates and realized R by candidate-source; retire if dominated) |
| Conviction-scaled sizing relies on calibrated A/B/C; pre-P4 calibration may be sparse | P1 (cap conviction multiplier at 1.0× until P4 calibration shows A → realized hit-rate > target with N ≥ 50) |
| AV fundamentals fallback flakiness; manual store + IBKR news may suffice | P5 (audit which fundamentals fields are load-bearing for blackout/sizing; if AV unused, retire) |

## Removals + corrections committed in this program

The user has approved removing/correcting features that don't earn their keep. Each is owned by a phase:

| Item | Action | Phase |
|---|---|---|
| `score = clamp(net_pnl/100, ±25)` grade formula | **Replace** with `Σ(realized_R × conviction_weight) - discipline_penalty`, surfaced as `score_v2` and `discipline_v2` separately (not a single opaque scalar) | P4 |
| Hardcoded `target_2r` / `target_3r` on `SetupCandidate` | **Replace** with `targets: Vec<TargetSpec>` driven by per-detector ATR-multiple config + bracket scale spec | P7 |
| Static detector parameters in `settings.toml` | **Augment** with `param_vintage` field; vintaged params win at runtime, settings become floor/ceiling bounds | P10 |
| Sentiment-surge scanner (Apewisdom + Reddit + Stocktwits) | **Audit then decide** — retire if backtest shows source dominated by IBKR scanner; demote to advisory-only if mixed | P6 |
| LLM thesis prose | **Audit, do not auto-retire** — measure outcome lift; if none, mark optional but keep on by default for discipline value | P6 |
| AV fundamentals fallback | **Audit then decide** — if no consumer is load-bearing, retire entire AV path including ledger + cache + rate limiter | P5 |
| Detectors that fail OOS profit-factor > 1.2 over 18 months | **Disable by default** in `settings.toml`; user can re-enable with eyes-open flag | P6 |

Each phase that owns a removal carries the deletion in its diff. No phase removes a feature that earlier phases haven't already disconnected.
