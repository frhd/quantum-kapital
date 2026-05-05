# Phase 2 — compute service (TWR / MWR / drawdown) + read Tauri commands

> Part of [Portfolio performance analytics](master.md). See index for invariants.

**Status:** todo

**Depends on:** 1

**Goal:** Add the pure-math layer over Phase 1's snapshot store and expose it through two read Tauri commands. After this phase the backend can answer "what is my equity curve?" and "what are my headline metrics?" deterministically over any window. No IO outside `repository.rs` (re-used from Phase 1).

## Files

- New: `src-tauri/src/services/performance/compute.rs` — `equity_curve(snapshots)`, `twr(snapshots, cash_flows)`, `mwr_annualized(snapshots, cash_flows)`, `max_drawdown(snapshots)`, `daily_return_extremes(snapshots)`, `annualized_volatility(snapshots)`, `compute_metrics(snapshots, cash_flows)`. All pure; no `Db` access. Stays under 250 lines.
- New: `src-tauri/src/services/performance/irr.rs` — `xirr(flows: &[(NaiveDate, f64)]) -> Option<f64>`. Hand-rolled Newton-Raphson with bisection fallback on `[-0.999, 10.0]`. ~80 lines including tests; no new crate.
- Touches: `src-tauri/src/services/performance/types.rs` — add `EquityPoint { date, net_liquidation, return_from_start_pct, drawdown_pct }`, `PerformanceMetrics { current_net_liquidation, total_return_pct, total_return_abs, annualized_return_pct, max_drawdown: DrawdownStats, best_day_pct, worst_day_pct, daily_volatility_annualized, days_tracked, tracking_since: NaiveDate, currency }`, `DrawdownStats { peak_date, trough_date, drawdown_pct, drawdown_abs, recovered: bool }`, `PeriodWindow { OneWeek, OneMonth, ThreeMonths, Ytd, OneYear, All, Custom { from, to } }`.
- Touches: `src-tauri/src/services/performance/mod.rs` — expose `PerformanceService::get_curve(window, account?) -> Result<Vec<EquityPoint>>` and `get_metrics(window, account?) -> Result<PerformanceMetrics>`. Both methods: load snapshots + cash_flows from the repository for the given window, hand to `compute.rs`, return.
- Touches: `src-tauri/src/ibkr/commands/performance.rs` — add `performance_get_curve(window, account?)` and `performance_get_metrics(window, account?)` Tauri commands.
- Touches: `src-tauri/src/ibkr/commands/mod.rs` — re-export the two new commands.
- Touches: `src-tauri/src/lib.rs` (`run`) — register the two new commands in the `tauri::generate_handler![...]` macro list.

## Tools / endpoints exposed

| Command | Wraps |
|---|---|
| `performance_get_curve(window, account?)` | `PerformanceService::get_curve` → `Vec<EquityPoint>` for the requested window. Empty vec if no snapshots. |
| `performance_get_metrics(window, account?)` | `PerformanceService::get_metrics` → `PerformanceMetrics` (or `null` envelope when 0 snapshots). |

## Reuse (no new business logic this phase)

- `repository::snapshots_in_range`, `repository::cash_flows_in_range` from Phase 1.
- `chrono::NaiveDate` arithmetic (already used throughout the codebase).
- `serde` derives + the existing Tauri command-error pattern (`Result<T, String>` with `.map_err(|e| e.to_string())`).
- No new crate. The hand-rolled XIRR replaces what `roots`/`argmin` would offer; one function, well-tested.

## Decisions to make in this phase

- **TWR sub-period boundaries.** A cash flow on date `d` splits the day. v1 convention: flow happens at start of day, so `R_d = NetLiq_d / (NetLiq_{d-1} + CF_d) − 1`. Documented in `compute.rs` doc-comment. With empty `&[CashFlow]` (the v1 case), `twr` short-circuits to `V_end / V_start − 1`.
- **MWR / annualized return.** `mwr_annualized(snapshots, cash_flows)`:
  - Empty cash flows → `(V_end / V_start) ^ (365 / Δcalendar_days) − 1` (CAGR closed form). No XIRR call.
  - Non-empty cash flows → build the flow vector `[(date_0, -V_start), ...cash_flows..., (date_n, +V_end)]` and call `xirr(...)`. Return `None` on solver failure rather than panicking.
- **Annualization basis.** 365 calendar days for CAGR; 252 trading days for daily-return stdev → annualized vol.
- **XIRR initial guess.** `0.10` (10%). Bisection bracket `[-0.999, 10.0]`. Max 100 Newton iterations; fall back to bisection if the step diverges or oscillates.
- **Best/worst day units.** `pct` only on the metric struct; absolute `$` exposed via tooltip (not part of `PerformanceMetrics`).
- **`PeriodWindow::All` semantics.** No date filter — returns every snapshot. `Ytd` floors the start at Jan 1 of the current ET year. `1Y` is `today − 365 days`.
- **`PerformanceMetrics` for sparse data.** With 0 snapshots, return `null` from the Tauri command (encoded as `Option<PerformanceMetrics>` on the Rust side). With 1 snapshot, return `Some(...)` with `total_return_pct = 0`, `annualized_return_pct = None`, `max_drawdown = DrawdownStats::flat(...)`, `best_day_pct = None`, `worst_day_pct = None`, `daily_volatility_annualized = None`, `days_tracked = 1`. The UI handles the `Option` fields.

## Exit criteria

- `cargo test services::performance::compute` green. At minimum:
  - `twr` with synthetic flat curve = 0.
  - `twr` with `+10% / -10%` pair = `-1%` (geometric link).
  - `twr` with empty cash flows == `V_end / V_start − 1` (closed-form check).
  - `mwr_annualized` with zero cash flows == CAGR closed form to ±1e-9.
  - `max_drawdown` on a known peak-trough-recovery sequence: peak/trough dates correct, `recovered` flag flips when the curve climbs back to peak.
  - `daily_return_extremes` finds the best/worst day in a fixture.
  - `annualized_volatility` on `[1.0, 1.01, 1.0, 1.01]` == sample stdev × √252 to ±1e-9.
- `cargo test services::performance::irr` green:
  - Three known-answer XIRR cases pinned to ±1e-7 against Excel/Sheets reference values.
  - Single-flow input → `None` (no bracketing root).
  - Negative-IRR case bracketed and converged.
- From a dev console: `performance_get_metrics({ window: 'all' })` returns a populated struct on a seeded DB. JSON round-trips cleanly through serde.
- Tracer-bullet: 5 hand-crafted snapshots → `total_return_pct ≈ +3.5%`, `max_drawdown_pct ≈ -1.6%`, `best_day_pct ≈ +3.7%`, `worst_day_pct ≈ -1.4%`, `days_tracked = 4`. Cross-checked against a spreadsheet.
- Pre-commit clean.
- Every new Rust file < 300 lines.

## Gotchas

- **Sparse curve.** The curve only has trading-day points (weekends/holidays are gaps). CAGR uses **calendar** days between first and last snapshot. Daily-return stdev uses sequential snapshot pairs — gaps count as one period each, which is correct for trading-day-spaced data. Don't try to interpolate weekends.
- **Single snapshot.** Most metrics are `None`; the UI must handle. Don't crash. Don't return zeros that look like real data.
- **Zero-variance period.** If every daily return is identical, vol is `Some(0.0)`, not `None`.
- **XIRR edge cases.** Single flow → no root in any bracket → return `None`. All-positive or all-negative flows → no IRR → return `None`. Don't panic.
- **Empty cash flows + closed-form.** Always short-circuit. XIRR with no flows is mathematically undefined; calling it would either panic or return garbage. The closed-form fallback is faster and correct.
- **`PeriodWindow::Custom`.** Validate `from <= to`; otherwise return `Err("invalid range")` from the command.
- **Account aggregation.** v1 read commands return per-account slices when `account` is `Some(...)`, and aggregate (sum NetLiq across all accounts on each date) when `account` is `None`. The aggregate path requires that every account has a snapshot for that date — if some are missing, drop the date from the aggregate curve and log a warning (don't fabricate values).
- **Mixed currency in aggregate.** If snapshots in a window span more than one currency, both `get_curve` and `get_metrics` return an error envelope `Err("mixed_currency_unsupported_v1")` so the UI can show the warning banner. Per-account calls remain correct.
- **Floating-point drift in TWR.** Geometric link can accumulate tiny errors over 1000+ days. v1 acceptable; if it ever bites, switch to log-return summation.
- **Don't forget to register both Tauri commands in `lib.rs`** — easy miss; the frontend will get `command not allowed` if you skip.
