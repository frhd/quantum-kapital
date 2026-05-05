# Phase 6 — Backtest harness

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** todo

**Depends on:** 2

**Goal:** Replay `bars_cache` through the live `DetectorRegistry` to compute realized R per signal, with a fill model calibrated from P2 slippage data. Validate every detector. Decide which to retire, which to refit (P10), which to gate by regime (P9). Without this, every detector parameter is opinion.

This is the largest single phase. Plan two weeks.

## Files

- New: `src-tauri/src/services/backtester/mod.rs` — `Backtester::run(BacktestSpec) -> BacktestResult`.
- New: `src-tauri/src/services/backtester/replay.rs` — Bar-by-bar replay against `DetectorRegistry`. Provides `MarketContext` reconstructed from `bars_cache`.
- New: `src-tauri/src/services/backtester/fill_model.rs` — `FillModel` trait with two impls: `NaiveNextOpenFill` (next-bar-open + fixed bps) and `CalibratedFillModel` (per-strategy slippage distribution sampled from P2 historical fills).
- New: `src-tauri/src/services/backtester/results.rs` — `BacktestResult { trades, daily_equity, metrics, by_strategy, by_regime, ... }`. Reuses P4 `RiskMetrics` shape.
- New: `src-tauri/src/services/backtester/walk_forward.rs` — Walk-forward splits: train window → OOS window → roll. Outputs OOS-only metrics.
- New: `src-tauri/src/services/backtester/spec.rs` — `BacktestSpec { date_range, symbols, detectors, fill_model, splits, position_sizing }`.
- New: `src-tauri/src/storage/migrations/` — `backtest_runs` table: `run_id`, `spec_json`, `result_json`, `created_at`. `backtest_trades` table indexed by `run_id`.
- New: `src-tauri/src/ibkr/commands/backtest.rs` — `backtest_run(spec)`, `backtest_get_run(run_id)`, `backtest_list_runs(filter)`.
- New: `src/features/backtest/components/BacktestRunner.tsx` — UI: configure spec, kick off run, observe progress.
- New: `src/features/backtest/components/BacktestResults.tsx` — Equity curve, per-detector / per-regime / per-month breakdown.
- New: `src-tauri/src/bin/qk-backtest.rs` — CLI entry point for headless / overnight runs.
- Touches: `src-tauri/src/services/historical_data_service/` — Add `BarsReader` trait method for bulk window read; backtester uses it without touching IBKR.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `backtest_run` | Kick off a run. Returns `run_id` immediately; results stream via `BacktestProgress` event. |
| `backtest_get_run` | Read full result JSON. |
| `backtest_list_runs` | List recent runs. |
| `backtest_compare` | Diff two runs (e.g., baseline vs. with-blackouts). |

## Reuse

- `DetectorRegistry` and `MarketContext` are reused **as-is** — backtester is a different entry point that feeds them, not a fork. Any drift between live and backtest is a bug.
- P1 `RiskEngine::size` for backtest position-sizing (configurable: fixed-1R, conviction-scaled, or no-sizing-pure-R).
- P2 slippage distribution → `CalibratedFillModel`.
- P4 `RiskMetrics` and `equity_curve` — reuse the same struct shape.
- Existing `bars_cache` SQLite — read-only.
- Calendar utility for RTH gating in replay.
- P5 `EventCalendarService` so backtests honor event blackouts (configurable per spec).

## Decisions to make in this phase

- **Bar-replay determinism.** **Decision: strict point-in-time.** No look-ahead. Detectors only see bars with `bar_time <= current_replay_time`. News also gated on `fetched_at`.
- **Fill timing assumption.** Most detectors fire on bar-close. **Decision: fill at next bar's OPEN, not close. Slippage from `CalibratedFillModel`.** Trader can configure per-spec.
- **Position-sizing in backtest.** **Decision: support three modes** — fixed-shares (1R per trade), conviction-scaled R, or no-sizing-just-R. Default: conviction-scaled R, mirroring production.
- **News data in replay.** `news_cache.fetched_at` may be hours after news event publication. **Decision: replay uses `news_cache.published_at` for gating; if absent, uses `fetched_at` minus a fixed offset (default 30 min).** Document the assumption.
- **LLM thesis in backtest.** Generating real LLM theses for backtest is expensive. **Decision: backtester does NOT call LLM. Conviction defaults to `B` for any setup unless a stub conviction-from-numerics rule is provided.** This means backtest measures detector edge, not LLM-pick edge.
- **Walk-forward window sizes.** **Decision: 12-month train, 3-month OOS, 1-month roll.** Configurable per spec; min OOS window 1 month enforced.
- **Run scope.** **Decision: max 50 symbols per run; max 5 years history.** Beyond this, the run is rejected with a "split into smaller runs" error. Avoids OOM and keeps run-time reasonable.

## Exit criteria

- `cargo test backtester::` passes: replay determinism (rerun same spec → identical result hash), look-ahead detection (intentionally leaky test fails), fill-model edge cases (gap-up open fills above intended), walk-forward splitting correctness.
- End-to-end: run backtest of all 3 detectors on top-50 watchlist symbols × 18 months → results land in `backtest_runs` and render in UI.
- Calibrated fill model produces slippage distribution that matches P2 historical mean ± 1 bp.
- Backtest of P5 blackouts shows reduced earnings-bar trade count (sanity check).
- **Headline outputs decided and documented:**
  - Per-detector OOS profit factor over last 18 months.
  - Per-detector OOS Sharpe.
  - Per-detector OOS expectancy in R.
  - Sentiment-surge candidate-source vs. IBKR scanner candidate-source: realized R comparison (drives master-removal decision for sentiment-surge).
  - LLM-thesis-A/B (paired sample of setups; thesis vs. no-thesis): drives master removal decision for thesis prose.
- Detector retirement decisions documented in `QUESTIONS.md` Phase 6 section. Any detector with OOS profit-factor < 1.2 over 18 months is **disabled by default** in `settings.toml` in this phase's diff.

## Gotchas

- **Look-ahead is the default failure mode.** Indicator implementations that read `bars[i+1]` somewhere will silently inflate results. Test with a known-leaky detector to verify the harness catches it (assertion on bar-time monotonicity inside `MarketContext`).
- **Survivorship bias.** `bars_cache` is what the user has fetched, which skews to current watchlist (winners). Document this; do not claim universal edge from a watchlist-restricted sample.
- **Splits and dividends.** `bars_cache` from IBKR is split-adjusted; dividends are not adjusted in price. Document; for swing horizons of days, dividend impact is < 0.5% — accept and move on.
- **Commissions.** Fixed default $1/trade unless trader-configured. Don't model tiered.
- **Walk-forward with too few trades.** OOS Sharpe over a window with < 20 trades is meaningless. Surface "insufficient sample" instead of a number.
- **Runtime cost.** 18 months × 50 symbols × intraday 15m bars is ~1.6M bars. The replay must stream, not load all bars in memory. Index `bars_cache` on `(symbol, bar_size, bar_time)` and read in windows.
- **Reproducibility.** Result hash must be deterministic from spec_json. Random number sources (e.g., bootstrap sampling in `CalibratedFillModel`) seed from spec.
- **Forks of MarketContext.** Avoid duplicating MarketContext construction. If backtest needs a different shape, extend the shared trait, not a fork.
- **Don't break live.** Backtester must NEVER call `IbkrClient` (no live data). Trait seam enforces this; CI test asserts the binary doesn't link the live client.
- **CI cost.** Don't run full backtest in CI; use a tiny fixture (1 symbol, 30 days). Full runs are user-triggered or nightly cron.
