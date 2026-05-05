# Phase 9 — Regime gating

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** todo

**Depends on:** 6

**Goal:** Detectors declare preferred market regimes (e.g., breakout: trending + low-vol; parabolic short: post-spike + correction-prone). A regime classifier computes the current regime from VIX, SPY trend, breadth, and cross-sectional correlation. Off-regime detectors stay quiet. Validated by P6 backtest comparing on-regime vs all-regime stats.

## Files

- New: `src-tauri/src/services/regime/mod.rs` — `RegimeService::current() -> Regime`. Updates daily (close) and intraday (every 15 min during RTH).
- New: `src-tauri/src/services/regime/classifier.rs` — Maps observable signals → `Regime` enum.
- New: `src-tauri/src/services/regime/types.rs` — `Regime { trend: Up | Sideways | Down, vol: Low | Normal | High, breadth: Healthy | Mixed | Narrow, corr: Low | High }`. 4 axes × 3 levels = 81 possible regimes; in practice ~10 are common.
- New: `src-tauri/src/services/regime/inputs.rs` — Pulls SPY 50/200, VIX level + trend, breadth proxy (e.g., % SPX above 50DMA from a free source or computed from `bars_cache` for SP500 subset), correlation proxy.
- Touches: `src-tauri/src/strategies/trait_def.rs` — `StrategyDetector::preferred_regimes() -> RegimeFilter`. Default impl returns "all regimes" for backward compat.
- Touches: each detector — declare `preferred_regimes` based on P6 backtest evidence.
- Touches: `src-tauri/src/services/tracker_runner/` — Before running a detector, check `regime.matches(detector.preferred_regimes())`. If no, skip with `skipped_reason: OffRegime`.
- Touches: `src-tauri/src/storage/migrations/` — `regime_snapshots` table: `at`, `regime_json`, `inputs_json`. `setups.regime_at_decision_json TEXT`.
- New: `src/features/portfolio/components/RegimeIndicator.tsx` — Compact "Trend: Up / Vol: Normal / Breadth: Healthy" pill at top of screen.
- New: `src/shared/api/regime.ts`.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `regime_current` | Current regime + inputs. |
| `regime_history` | Time-series for charting. |
| `regime_force_recompute` | Manual recompute. |

## Reuse

- `bars_cache` for SPY, VIX, broad universe subset.
- P6 backtester for validation (on-regime vs. all-regime per-detector).
- Existing market calendar for "is market open" gating on intraday refresh.

## Decisions to make in this phase

- **Regime axes.** **Decision: 4 axes (trend, vol, breadth, correlation), each with 3 levels.** Coarseness is intentional; fine-grain regimes overfit.
- **Breadth source.** Free reliable sources for "% SPX above 50DMA" are limited. **Decision: compute from `bars_cache` for the top 200 SP500 names. Refresh daily at close. Falls back to "unknown" if < 80% of 200 have fresh bars; in unknown, regime axis defaults to `Mixed`.**
- **Correlation proxy.** **Decision: 20-day rolling avg pairwise correlation across top 50 SP500. Bucket: `Low < 0.5`, `High ≥ 0.5`.**
- **Per-detector regime preferences.** Initial defaults from P6 backtest; refined in P10. **Decision (initial):**
  - Breakout: `trend in {Up, Sideways}` AND `vol in {Low, Normal}`. Skip in `Down + High vol`.
  - Parabolic short: `vol in {Normal, High}` AND `trend != Up_strong`. Skip in `Up + Low vol` (no parabolas in melt-ups; but the underlying in question may still be parabolic — exception: the symbol-level parabolic test in the detector is the inner gate; regime is the outer).
  - Episodic pivot: regime-agnostic (all). Gap-on-news edge not believed regime-bound; will revisit if backtest disagrees.
- **Minimum trade frequency floor.** **Decision: per detector, if regime gating drops monthly trade count below 5 over a 12-month window, widen the regime envelope or retire.** Auto-checked monthly via P10 cron.
- **Override.** Trader can take an off-regime setup with logged reason (gate-override pattern from P5/P8 reused).

## Exit criteria

- `cargo test regime::` passes: classifier edge cases (VIX spike day, breadth missing data, holiday-truncated correlation).
- Backtest comparison committed to `QUESTIONS.md`: per-detector on-regime vs. all-regime profit-factor, Sharpe, expectancy in R. Decisions justified.
- Live: regime indicator visible at all times. Off-regime setups appear in skipped panel with "regime: trend=Down vol=High" annotation.
- Trade frequency floor check passes for all live detectors over a backtest 12-month window.
- Migration clean; pre-P9 setups have NULL `regime_at_decision_json`.

## Gotchas

- **Regime classification at signal time vs. exit time.** Setup is gated on regime at signal time only. Don't re-gate on regime change while setup is open — that's an exit-policy concern, not regime gating.
- **Whipsaw.** Daily classification can flip frequently around regime boundaries. Use a 3-day persistence rule for regime change (axis change must hold 3 sessions before flipping).
- **Backtest currency.** Regime classifier must be deterministic from `bars_cache` snapshot — same date range, same inputs, same regime sequence. Otherwise backtest comparisons are not reproducible.
- **Holiday truncation.** Breadth and correlation computed over rolling 20 sessions must skip holidays — already handled by calendar utility, but verify in tests.
- **Survivorship in breadth proxy.** The "top 200 SP500" set changes over time. For backtests, fix the set as-of the test date.
- **Don't add a 5th axis casually.** Each axis multiplies regime count. If P6 evidence demands a 5th (e.g., yield curve), document the case explicitly; don't add silently.
- **Episodic-pivot exception.** Initially regime-agnostic; if P6 reveals it's biased to a regime, fold in. Don't over-engineer here.
- **Per-detector preference must be testable from outside.** Add a unit test: `for each detector, for each regime, the gate behaves as declared`. Catches drift between code and config.
