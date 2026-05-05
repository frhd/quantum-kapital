# Phase 4 — R-adjusted grading + risk metrics

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** todo

**Depends on:** 1, 2

**Goal:** Replace `score = clamp(net_pnl/100, ±25) + Σ(tag_weights)` — a formula that conflates edge with discipline and ignores risk taken — with two distinct surfaced numbers: an **R-edge score** and a **discipline score**. Add the standard risk-adjusted suite (Sharpe, Sortino, Calmar, profit factor, expectancy, max DD, equity curve). Per-strategy attribution becomes a first-class view, not a derivation buried in SQL.

## Files

- Touches: `src-tauri/src/services/trade_reviews/grading.rs` — Replace formula. New: `score_v2 = Σ(realized_R × conviction_weight)`. New: `discipline_v2 = Σ(tag_weights)`. Composite shown but never summed for ranking.
- New: `src-tauri/src/services/trade_reviews/risk_metrics.rs` — `RiskMetrics { sharpe, sortino, calmar, profit_factor, expectancy_r, max_dd, max_dd_duration, win_rate, avg_win_r, avg_loss_r }`. Computed from a daily equity series.
- New: `src-tauri/src/services/trade_reviews/equity_curve.rs` — Reconstructs daily equity series from `executions`. Pure function; deterministic; testable against fixtures.
- Touches: `src-tauri/src/services/trade_reviews/mod.rs` — `TradeReviewGenerator` orchestrates the new fields; writes `score_v2`, `discipline_v2`, `risk_metrics_json` columns alongside legacy `score` (for backward read).
- New: `src-tauri/src/services/trade_reviews/attribution.rs` — Per-strategy view via TCA join (P2). Outputs `StrategyRollup { strategy, n_trades, realized_pnl, avg_r, win_rate, profit_factor, sharpe_30d }`.
- Touches: `src-tauri/src/storage/migrations/` — `day_reviews` adds: `score_v2 REAL`, `discipline_v2 REAL`, `risk_metrics_json TEXT`, `equity_curve_json TEXT`, `formula_version TEXT`.
- New: `src/features/trade-review/components/RiskMetricsPanel.tsx` — Sharpe / Sortino / Calmar / PF / expectancy / DD card grid.
- New: `src/features/trade-review/components/EquityCurve.tsx` — D3-or-recharts daily equity line + DD shading.
- New: `src/features/trade-review/components/StrategyRollup.tsx` — Table per detector.
- Touches: `src/features/trade-review/components/DayReviewCard.tsx` — Show `score_v2` + `discipline_v2` separately, NOT a single composite. Old `score` shown only for pre-P4 review rows (with "v1" badge).
- New: `src/shared/api/tradeReviewMetrics.ts` — `trade_review_get_metrics`, `trade_review_get_strategy_rollup`, `trade_review_get_equity_curve`.
- Touches: `agent/eod_review.py` and `agent/trade_review.py` — Update prompts: narrative reads `score_v2` + `risk_metrics`; tags still drive `discipline_v2`.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `trade_review_get_metrics` | RiskMetrics over a date range. |
| `trade_review_get_strategy_rollup` | Per-strategy attribution (P2 join). |
| `trade_review_get_equity_curve` | Daily equity series for charting. |

## Reuse

- P2 `executions.setup_id` and `executions.strategy` for attribution.
- P2 `trade_legs` for round-trip aggregation.
- Existing `day_reviews` table — add columns, do not replace.
- Existing behavioral tag taxonomy and weights — `discipline_v2` reuses them unchanged.
- Existing `services/calibration_stats` for conviction-vs-outcome inputs into score (`conviction_weight` is calibrated, not hardcoded).

## Decisions to make in this phase

- **Conviction weights.** Hardcoded vs calibrated from `calibration_stats`. **Decision: calibrated.** `conviction_weight = realized_target_rate(conviction) / target_rate(C)`. Fallback to 1.0 if N < 50 for that conviction; log fallback.
- **Risk-free rate for Sharpe.** **Decision: 4.5% annualized as default; configurable.** Recompute on settings change.
- **Trading-days-per-year.** **Decision: 252.** Used for annualization of Sharpe / Sortino.
- **Equity curve reconstruction edge cases.** Deposits, withdrawals, dividends, fees not from trades. **Decision: subtract from daily PnL series via `account_summary` delta vs prior NLV minus trade PnL; if mismatch > $50, mark day as "reconciliation_warning" and exclude from Sharpe.** User-overridable.
- **Strategy rollup grain.** Per detector class only, OR per detector × symbol-liquidity bucket. **Decision: detector class only this phase; finer grain after P10.**
- **Discipline score sign convention.** Tag weights are negative for bad behavior. **Decision: `discipline_v2 = Σ(tag_weights)`; range typically -30 to 0; surface as a deficit, not a positive.**
- **Composite ranking.** Some UI panels need a single "best day / worst day" sort. **Decision: rank by `score_v2` only; show `discipline_v2` as secondary column. Never sum the two.**

## Exit criteria

- `cargo test trade_reviews::` passes new tests covering: equity-curve reconstruction with deposits/withdrawals, Sharpe over fixture series matches reference (within 1e-6), profit-factor / expectancy edge cases (no-loss, no-win, all-loss).
- Integration test: 30 days of fixture executions → `trade_review_get_metrics` returns expected RiskMetrics.
- `score_v2` and `discipline_v2` populated for all post-P4 day reviews; pre-P4 rows untouched (immutable, `formula_version` = "v1").
- Frontend Risk Metrics panel renders against real data; equity curve charts; strategy rollup shows per-detector PnL and Sharpe.
- CI grep: `net_pnl\s*/\s*100` does not appear in `services/trade_reviews/` (test fixtures may reference for backcompat reads).
- Tracer-bullet test (master cross-phase verification): a P3-placed bracket fills → `executions.setup_id` recorded (P2) → `trade_legs.strategy` populated → `score_v2` reflects realized R, not net_pnl/100. End-to-end across `MockIbkrClient`.

## Gotchas

- **Equity curve daily granularity is wrong for intraday review.** This phase commits to daily; intraday curve is out of scope.
- **Profit-factor with zero losses is undefined.** Return `f64::INFINITY` for that case and surface as "—" in UI; don't crash, don't return 0.
- **Sharpe over short windows is noise.** UI must require N ≥ 20 trading days before showing the number; below that show "insufficient history."
- **Backward read of pre-P4 reviews.** `DayReviewCard` reads `score_v2 IS NULL → fall back to score with v1 badge`. Do not retroactively recompute v1 rows.
- **Conviction-weight calibration coupling.** Calibration stats lag (30-day window). Phase-1 sizing uses conviction; Phase-4 grading rewards conviction. If A miscalibrates upward, both sizing AND grade reward error. Phase 11 tilt guard exists partly to clamp this risk; document the coupling explicitly in code comments at the conviction_weight call site.
- **Agent prompt drift.** `agent/eod_review.py` and `agent/trade_review.py` write narratives that reference grade fields. Update both, run a smoke test against last 30 days of fixture data.
- **`day_reviews.formula_version`.** Set to `"v1"` for any pre-P4 row read, `"v2"` for new writes. Never silently upgrade an old row.
- **MCP read-only invariant.** `get_trade_review` MCP tool should expose new fields but never `score` v1 alone for a v2-eligible date — return both with version flag.
