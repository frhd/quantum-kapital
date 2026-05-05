# Phase 3 — Performance tab UI

> Part of [Portfolio performance analytics](master.md). See index for invariants.

**Status:** todo

**Depends on:** 2

**Goal:** Add a new sidebar tab **Performance** that surfaces the equity curve, KPI cards, and a small drawdown chart over the data Phases 1–2 expose. Installs `recharts`. Reuses `Card` / `Button` / `Badge` primitives and the `useTrades`-style data hook pattern.

## Files

- New: `src/features/performance/components/PerformanceTab.tsx` — top-level layout: title row + period selector → metrics row (six cards) → main equity-curve chart card → small drawdown chart card → footer ("tracking since YYYY-MM-DD; N trading days").
- New: `src/features/performance/components/MetricsCards.tsx` — six `Card`s: Current NetLiq, Total Return ($ + %), Annualized Return (CAGR), Max Drawdown, Best Day %, Worst Day %.
- New: `src/features/performance/components/EquityCurveChart.tsx` — `recharts.AreaChart` with gradient fill; X = date, Y = NetLiq; tooltip shows date / NetLiq $ / day return % / drawdown %; toggle button switches Y between `$ value` and `% return from start`.
- New: `src/features/performance/components/DrawdownChart.tsx` — small red `AreaChart` (value ≤ 0) showing rolling drawdown.
- New: `src/features/performance/components/PeriodSelector.tsx` — segmented buttons: 1W / 1M / 3M / YTD / 1Y / All. Controlled component; emits `PeriodWindow` to the parent.
- New: `src/features/performance/hooks/usePerformance.ts` — `useState` + `useCallback` + `useEffect` mirroring `src/features/trades/hooks/useTrades.ts`. Window-focus + visibilitychange refetch. Returns `{ curve, metrics, loading, refreshing, error, refresh, captureNow }`.
- New: `src/features/performance/types.ts` — TS mirrors of the Rust DTOs (`EquityPoint`, `PerformanceMetrics`, `DrawdownStats`, `PeriodWindow` union).
- Touches: `src/shared/components/layout/Sidebar.tsx` — extend `PageId` with `"performance"`, insert `{ id: "performance", label: "Performance", icon: TrendingUp }` between `Positions` and `Account` inside the `Account` group, add `performance: "Performance"` to `PAGE_LABELS`.
- Touches: `src/App.tsx` — add `{currentPage === "performance" && <PerformanceTab />}` in the render switch.
- Touches: `src/shared/api/ibkr.ts` — append `performance: { getCurve, getMetrics, captureSnapshotNow }` block alongside the existing `trades` block. Typed return values from `src/features/performance/types.ts`.
- Touches: `package.json` — add `recharts` (`pnpm add recharts`).

## Tools / endpoints exposed

None new at the backend layer (Phases 1–2 already shipped them). Frontend wrappers in `shared/api/ibkr.ts`:

| Wrapper | Wraps |
|---|---|
| `ibkrApi.performance.getCurve(window, account?)` | `invoke<EquityPoint[]>("performance_get_curve", { window, account })` |
| `ibkrApi.performance.getMetrics(window, account?)` | `invoke<PerformanceMetrics \| null>("performance_get_metrics", { window, account })` |
| `ibkrApi.performance.captureSnapshotNow(account?)` | `invoke<AccountSnapshot>("performance_capture_snapshot_now", { account })` |

## Reuse (no new business logic this phase)

- `src/shared/components/ui/{Card, Button, Badge}` — every card and button reuses these.
- `src/features/trades/hooks/useTrades.ts` as the template for `usePerformance.ts` — same window-focus refetch wiring, same `loading` vs `refreshing` distinction.
- `src/features/trades/components/TradesPage.tsx` as the template for the tab-content shell (Card + CardHeader + CardContent layout, padding, scroll).
- Tailwind 4 utility classes only — no CSS modules.
- Theme CSS vars from `src/styles/index.css` for chart colors (so dark mode keeps working).

## Decisions to make in this phase

- **Single-account vs aggregated view.** v1 aggregates all accounts (sum NetLiq, per-date) when `account` is omitted. A `(All accounts)` chip is shown next to the title. No account switcher in v1.
- **Chart Y-axis mode default.** `$ value` (more intuitive for first-time users). Toggle button in the chart-card header flips to `% return from start`.
- **Period selector default.** `All`.
- **Empty-state copy.**
  - 0 snapshots: centered card with "No snapshots yet — performance starts being tracked at today's market close." + primary `[Capture snapshot now]` button that calls `captureSnapshotNow()` and refetches.
  - 1 snapshot: centered card with "1 snapshot recorded — check back tomorrow for a curve." Metrics row hidden; chart shows a single dot with date label.
  - ≥2 snapshots: full chart and metrics.
  - Mixed-currency error envelope: warning banner "Mixed-currency portfolios aren't yet supported. Switch to a single-currency account view to see metrics." Chart and metrics hidden.
- **Refetch cadence.** Initial load on mount; refetch on window focus + visibilitychange; refetch after `captureSnapshotNow()` resolves; refetch when `period` changes.
- **MetricsCards labels.**
  - `Total Return` (subtitle `since YYYY-MM-DD`, value `+3.45% ($1,250)`).
  - `Annualized Return (CAGR)` with a tooltip: *"Equivalent to MWR while no external cash flows are tracked. Once deposits/withdrawals are recorded, this will diverge from TWR."*
  - `Max Drawdown` (subtitle `peak YYYY-MM-DD → trough YYYY-MM-DD`, value `-1.6%`).
  - `Best Day %` (subtitle `YYYY-MM-DD`).
  - `Worst Day %` (subtitle `YYYY-MM-DD`).
  - `Current NetLiq` (subtitle `as of YYYY-MM-DD`).
- **Color convention.** Profit positive → green from theme; loss negative → red; neutral → muted. Match `StockPositions.tsx`'s P&L coloring.

## Exit criteria

- `pnpm tauri dev` opens the app; clicking **Performance** in the sidebar routes to the new tab without navigation glitches.
- Empty state, 1-snapshot state, and N-snapshot state (verified by directly inserting synthetic rows into `account_snapshots`) all render cleanly.
- Period selector switches between 1W / 1M / 3M / YTD / 1Y / All without re-mounting the chart (recharts handles smooth transitions).
- Tooltip on chart hover shows date / NetLiq $ / day return % / drawdown %.
- Y-axis toggle flips between `$ value` and `% return from start` and updates the tooltip units.
- `Capture snapshot now` button on the empty state flips the tab to the 1-snapshot state without a manual reload.
- Dark mode: chart colors track the theme — confirm by toggling theme in dev tools.
- `pnpm prettier --check . && pnpm eslint .` clean.
- Every new TSX file ≤ 200 lines (soft cap); no file exceeds the 350-line hard cap.

## Gotchas

- `recharts` `<ResponsiveContainer>` requires a sized parent. The chart card must have explicit `h-80` (or similar) on the wrapping `<div>`. Without it, the chart renders at 0 px and silently disappears.
- Date formatting on the X-axis: use `date.toLocaleDateString()` for sparse axes; for dense axes show only month/year via `<XAxis tickFormatter>`. The `recharts` default is verbose ISO timestamps and looks awful.
- Dark mode: chart colors must come from CSS vars (`var(--color-foreground)`, `var(--color-primary)`, etc.) — passed into `<Area stroke={...}>` and `<Tooltip contentStyle={{ backgroundColor: 'var(--color-popover)' }}>` — so the existing theme switch keeps working without extra wiring.
- The `Annualized Return` KPI card label says `Annualized Return (CAGR)` in v1. **Don't** ship a literal `MWR` label until cash flows are wired up — it would mislead.
- Sidebar group ordering: the `Account` group already contains `Positions`, `Trades`, `Account`. Insert `Performance` between `Positions` and `Account` so the order reads "Positions → Trades → Performance → Account". Don't append at the end.
- Don't import `invoke` directly from any component — only via `shared/api/ibkr.ts`. Project rule. ESLint may not catch this; reviewer will.
- `usePerformance` should distinguish `loading` (initial fetch, full skeleton) from `refreshing` (subsequent fetch, in-place update with a small spinner). Mirror `useTrades`'s pattern; don't reinvent.
- `recharts` `<Area type="monotone">` interpolates between points; for trading-day-spaced data this looks fine, but if a long gap (e.g. 4-day weekend) shows up it can mislead. Acceptable for v1; revisit if a user complains.
- Tooltip flicker on rapid mouse moves: set `<Tooltip isAnimationActive={false}>` to avoid jitter on small charts.
- Number formatting: use `Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' })` for `$` values and `Intl.NumberFormat('en-US', { style: 'percent', minimumFractionDigits: 2 })` for `%` values. Don't roll your own.
- The `[Capture snapshot now]` button must be guarded against double-clicks — disable while `refreshing === true`.
