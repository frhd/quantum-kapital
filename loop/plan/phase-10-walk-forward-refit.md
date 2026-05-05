# Phase 10 — Walk-forward parameter refit

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** todo

**Depends on:** 6

**Goal:** Stop running detectors on hand-picked, never-revisited parameters. Monthly cron refits each detector's free parameters on a rolling 12-month window, OOS-tested, locks the chosen vintage for next month. Every setup at runtime carries `param_vintage` so reviews and backtests can attribute by parameter set, not just detector class.

## Files

- New: `src-tauri/src/services/param_refit/mod.rs` — `ParamRefitService::run_monthly() -> RefitReport`.
- New: `src-tauri/src/services/param_refit/sweep.rs` — Grid + random-search over each detector's parameter space; budget-bounded.
- New: `src-tauri/src/services/param_refit/objective.rs` — Objective function: OOS profit factor with min-trade-count guardrail and a complexity penalty (favor fewer adjustments to current vintage).
- New: `src-tauri/src/services/param_refit/vintage_store.rs` — Persists locked params per detector + month.
- Touches: `src-tauri/src/strategies/registry.rs` — At construction time, load latest vintage from store; fall back to `settings.toml` defaults if no vintage exists.
- Touches: `src-tauri/src/storage/migrations/` — `param_vintages` table: `id`, `detector`, `params_json`, `objective_value`, `oos_n_trades`, `train_window`, `oos_window`, `locked_at`, `superseded_at`. `setups.param_vintage_id INTEGER`.
- New: `src-tauri/src/services/param_refit/scheduler.rs` — Monthly cron at start of each calendar month, after market close on the previous month's last trading day.
- New: `src/features/eval/components/ParamVintageHistory.tsx` — Per-detector timeline of locked vintages and their OOS metrics.
- New: `src/shared/api/paramRefit.ts`.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `param_refit_run_now` | Manual trigger of monthly refit. |
| `param_refit_history` | Per-detector vintage history. |
| `param_refit_get_active` | Current active vintage per detector. |
| `param_refit_lock_manual` | Admin: lock a manual params override. |

## Reuse

- P6 backtester is the workhorse — sweep just calls `Backtester::run` with varied specs.
- P9 regime gating respected during refit (regime-conditional params optional but recommended).
- Existing settings.toml as floor/ceiling bounds for sweep.

## Decisions to make in this phase

- **Sweep budget.** **Decision: 200 backtest runs per detector per month max.** Random search over parameter space inside `settings.toml` bounds; grid for cheap params (volume_mult, RSI ceiling); random for ranges.
- **Objective.** **Decision: maximize OOS profit factor with constraints — min 30 OOS trades over 3 months; OOS Sharpe ≥ 0.5; expectancy in R ≥ 0.1.** Vintages that fail constraints don't unseat the current.
- **Lock condition.** **Decision: replace current vintage only if new candidate beats current by ≥ 10% on objective AND meets all constraints.** Avoids churn.
- **Settings.toml semantics shift.** **Decision: settings.toml becomes the *bounds* (min/max) for refit, not the active params.** Active = vintage. Migrate existing settings on first refit.
- **Regime-conditional vintages.** **Decision: phase 10 ships per-detector single-vintage; per-(detector × regime) vintage deferred** unless P6 evidence is overwhelming. Document deferral.
- **Backfill trigger for missing vintages.** **Decision: at startup, if a detector has no active vintage, run a one-shot refit immediately (don't wait for next month).** Avoids fresh-install dead detectors.
- **Refit failure mode.** Network down, IBKR down, bars_cache stale. **Decision: refit fails with a logged warning; current vintage stays active. Surface in eval panel.**

## Exit criteria

- `cargo test param_refit::` passes: sweep determinism (same seed → same vintage), constraint enforcement (vintage that fails N-trades is rejected), lock-on-improvement guard (new must beat by ≥ 10%).
- End-to-end: monthly cron runs against fixture bars; produces a vintage row per detector; setups created in next month carry `param_vintage_id`.
- Per-detector vintage history visible in UI.
- Migration clean; pre-P10 setups have NULL `param_vintage_id`.
- Backtest invariant test passes: every detector active in production has an OOS backtest entry within last 30 days (master cross-phase check).

## Gotchas

- **Multiple-comparison overfit.** 200 runs per detector × N detectors is a lot of comparisons. Constraints (min trades, min Sharpe) act as guards but are not airtight. Mitigation: out-of-sample window must be at least 1/3 of train window, and report all attempted configs in the vintage record so reviewers can see if the winner is luck.
- **Backtester runtime budget.** 200 runs × full 18 months × 50 symbols is hours of compute. Schedule for off-hours; cap per-run at smaller scope (3 months train + 1 month OOS) and validate the chosen vintage on full 18 months only after selection.
- **Vintage drift risk.** Frequent vintage changes break the trader's mental model. Lock-on-improvement (≥ 10% beat) is the friction that prevents this.
- **Settings.toml edits during a vintage's life.** If trader edits `settings.toml` to widen bounds, vintage stays active until next refit. Document; don't re-trigger on settings edit.
- **Concurrent backtest contention.** Refit runs N backtests; user-triggered backtests via P6 UI compete. Use a semaphore with bounded concurrency (e.g., 2 concurrent runs).
- **`SetupCandidate.param_vintage_id` propagation.** Threading the active vintage through from registry construction to setup persistence touches several modules; missing it breaks attribution. Add an integration test that asserts non-NULL `param_vintage_id` on every post-P10 setup.
- **Timezone of "monthly cron".** Use US/Eastern, run on the last trading day of the month after close. Calendar utility already supports last-trading-day computation.
- **Sentiment-surge candidate-source decision (from master).** P10 is the last phase that touches detector / signal config; if P6 deferred the sentiment-surge retirement, decide in P10's diff.
