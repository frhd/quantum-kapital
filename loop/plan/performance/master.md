# Portfolio performance analytics: equity curve + TWR/MWR — ~1 week

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a new desktop **Performance** tab that shows the user's portfolio equity curve plus headline return metrics (total return, annualized return / CAGR, max drawdown, best/worst day, daily volatility). Backed by daily account snapshots persisted to SQLite — forward-only — with the schema shaped so a future FlexQuery backfill can write retroactive rows without migration.

**Architecture:** Three layers. **L0 — daily snapshots** (`account_snapshots` table; one row per `(account, date)`; written by an EOD scheduler hook + a startup catch-up). **L1 — compute service** (pure functions over snapshot slices; TWR, XIRR, drawdown, daily-return moments; cash-flow-aware but defaulting to no flows in v1). **L2 — Tauri commands + UI tab** (one chart + KPI cards + period selector; recharts; reuses existing card primitives + `useTrades`-style data hook).

**Tech Stack:** Rust (rusqlite, refinery, tokio, serde), React 19 + TypeScript + Tailwind 4 + Vite + recharts (new dep).

---

## Context

The MCP and Tauri command surfaces today are tracker-centric — quotes, bars, news, setups, alerts, fills. Account aggregates (`NetLiquidation`, `TotalCashValue`, `GrossPositionValue`) are fetched live via `IbkrClient::get_account_summary` (`src-tauri/src/ibkr/client/mod.rs:343`) and surfaced in the Account tab, but never timestamped or persisted. There is no equity curve, no return calculation, no historical view.

IBKR's streaming API (`reqAccountUpdates`, `reqAccountSummary`) is current-values only. Historical NAV is only available through the FlexQuery REST endpoint, which is a separate auth + XML parsing integration not currently wired up. v1 locks to forward-only daily snapshots; FlexQuery is deferred.

Cash flows (deposits/withdrawals) are also unavailable: IBKR's streaming `AccountUpdate` types include cash-transaction events, but the adapter discards them (`src-tauri/src/ibkr/client/mod.rs:377`). v1 ships without a manual entry UI. With zero cash flows recorded:

- **TWR** over `[t0, t1]` reduces to `V(t1) / V(t0) − 1`.
- **MWR / annualized IRR** reduces to `(V(t1) / V(t0)) ^ (365 / Δdays) − 1` (CAGR).

These are still meaningful and worth showing; the UI labels them honestly so the user understands what's being computed.

**Inversion.** Today the app shows "this is your account, right now". End state: the app shows "this is how your account has evolved", with metrics that compound as more snapshots accumulate.

## End-state architecture

| Component | Layer | Responsibility |
|---|---|---|
| **`account_snapshots` table** (Phase 1) | L0 storage | One row per `(account, date)`; columns `net_liquidation`, `total_cash`, `gross_position_value`, `currency`, `source` (`'live_snapshot' \| 'flex_query'`), `captured_at`. UPSERT keyed on `(account, date)`. |
| **`cash_flows` table** (Phase 1) | L0 storage | Empty in v1. Schema present so future phases (manual entry / FlexQuery) write here without migration. Compute layer reads from it via a slice that defaults to empty. |
| **`PerformanceService::capture_snapshot`** (Phase 1) | L0 writer | Calls `IbkrClient::get_account_summary`, parses NetLiq/TotalCash/GrossPositionValue tags from string to `f64`, UPSERTs one row keyed `(account, today_et)`. |
| **EOD scheduler hook** (Phase 1) | L0 trigger | At each EOD scheduler tick on a trading day, capture snapshots for each connected account. On app startup, capture if today's row is missing (catch-up). |
| **`PerformanceService::get_curve` / `get_metrics`** (Phase 2) | L1 compute | Pure functions over `&[AccountSnapshot]` and `&[CashFlow]`. Equity curve points carry `date`, `net_liquidation`, `return_from_start_pct`, `drawdown_pct`. Metrics carry total return, annualized return, max drawdown, best/worst day, daily vol, days-tracked, tracking-since date. |
| **`xirr` solver** (Phase 2) | L1 compute | Hand-rolled Newton-Raphson + bisection fallback. Used when cash flows are present; with empty flows it short-circuits to CAGR. |
| **Tauri commands** (Phase 2) | L1 surface | `performance_get_curve(window, account?)`, `performance_get_metrics(window, account?)`, `performance_capture_snapshot_now(account?)` (the capture command lands in Phase 1; the read commands ship in Phase 2). |
| **`src/features/performance/`** (Phase 3) | L2 UI | New feature folder with `PerformanceTab`, `EquityCurveChart`, `MetricsCards`, `DrawdownChart`, `PeriodSelector`, `usePerformance` hook. Sidebar entry `Performance` under `Account`. |

## Hard invariants

1. **Surveillance-only stays.** No new code path places orders. The Performance tab is read-only end-to-end.
2. **All money values flow as `f64`** (existing convention, see `Position`, `AccountSummary` in `src-tauri/src/ibkr/types/`). No `Decimal` introduced.
3. **All dates in tool args are `YYYY-MM-DD`, US-Eastern trading day.** All wire timestamps are UTC ISO 8601. Frontend handles presentation TZ.
4. **Idempotency.** `account_snapshots` UPSERTs on `(account, date)` — re-running the EOD tick within the same day overwrites with the latest values; never duplicates.
5. **Schema forward-compat.** Both new tables include a `source` column from day one (`'live_snapshot' \| 'flex_query' \| 'manual'`) so a later FlexQuery ingester or manual cash-flow UI can co-exist without migration.
6. **Currency is recorded but assumed homogeneous in v1.** Every row stores a `currency` string. v1 only computes returns over rows where currency matches; mixed-currency portfolios show a warning banner and skip metrics. Multi-currency aggregation is deferred.
7. **No live IBKR in tests.** All Phase 1 tests use `MockIbkrClient` (`src-tauri/src/ibkr/mocks.rs`) for the snapshot writer. All Phase 2 tests use synthetic snapshot slices in-memory — no DB or IBKR.
8. **No new LLM call sites.** This program does not call an LLM.
9. **Pre-commit sacred.** `cargo fmt --check`, `cargo clippy -D warnings`, `prettier --check`, `eslint`. Never `--no-verify`.
10. **File-size caps.** Rust soft 300 / hard 500. TS/TSX soft 200 / hard 350. The `services/performance/` module is split (`mod.rs` + `types.rs` + `repository.rs` + `snapshot.rs` + `compute.rs` + `irr.rs`) from the start to stay well below caps.

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **Snapshot timing:** EOD only, ~16:01 ET on trading days (calendar via existing `utils/market_calendar`). Plus a single startup catch-up if today's row is missing.
- **Snapshot account scope:** every account currently visible to `IbkrClient`. One row per account per day.
- **Date column:** ISO 8601 `YYYY-MM-DD` ET trading day. Same convention as `executions.exec_time` (UTC) and `morning_packs.date` (ET).
- **Period windows:** `1W` (7 cal days), `1M` (30), `3M` (90), `YTD`, `1Y` (365), `All`. Applied as a half-open filter `(snapshot_date >= window_start)`.
- **Currency:** single-currency assumption v1; mixed → empty curve + warning.
- **Empty-state semantics:**
  - 0 snapshots → tab shows "No snapshots yet — performance starts being tracked at today's market close" + a `[Capture snapshot now]` button.
  - 1 snapshot → "1 snapshot recorded — check back tomorrow"; metrics hidden; chart shows a single dot.
  - ≥2 snapshots → full chart and metrics.
- **Sort order:** snapshots ascending by `date`. Curve points emitted in the same order.
- **Annualization basis:** 365 calendar days for CAGR; 252 trading days for daily-return-stdev → annualized vol.

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. `account_snapshots` schema + snapshot writer + EOD hook + capture-now command | [phase-1-snapshot-persistence.md](phase-1-snapshot-persistence.md) | — | todo |
| 2. Compute service (curve / TWR / XIRR / drawdown) + read Tauri commands | [phase-2-compute-and-commands.md](phase-2-compute-and-commands.md) | 1 | todo |
| 3. Performance tab UI (chart + KPI cards + period selector + sidebar wiring) | [phase-3-performance-tab-ui.md](phase-3-performance-tab-ui.md) | 2 | todo |

> **Status convention:** `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| Live account summary source | `src-tauri/src/ibkr/client/mod.rs` (`get_account_summary`, ~L343) |
| Account summary DTO | `src-tauri/src/ibkr/types/account.rs` |
| Existing daily-P&L stream (reference for streams pattern; not used here) | `src-tauri/src/ibkr/client/streams.rs` |
| MCP IBKR seam (read-only adapter) | `src-tauri/src/mcp/ibkr_seam.rs` |
| Storage migrations (next free: V14 — V13 is `__executions.sql`) | `src-tauri/src/storage/migrations/` |
| Storage runner + pool | `src-tauri/src/storage/migrations.rs`, `src-tauri/src/storage/mod.rs` |
| Service composition root | `src-tauri/src/lib.rs` (`run()`) |
| EOD scheduler (extend with snapshot hook) | `src-tauri/src/services/eod_scheduler/` |
| IBKR mock test seam | `src-tauri/src/ibkr/mocks.rs` |
| Reference service layout (mirror this) | `src-tauri/src/services/agent_morning_packs/`, `src-tauri/src/services/predictions/` |
| Reference Tauri commands (mirror these) | `src-tauri/src/ibkr/commands/accounts.rs`, `src-tauri/src/ibkr/commands/executions.rs` |
| Sidebar / nav registry | `src/shared/components/layout/Sidebar.tsx` (`PageId`, `NAV_GROUPS`, `PAGE_LABELS`) |
| App render switch | `src/App.tsx` |
| Tauri command wrapper module (extend) | `src/shared/api/ibkr.ts` |
| FE feature peers (mirror these) | `src/features/trades/`, `src/features/portfolio/` |
| FE UI primitives (reuse) | `src/shared/components/ui/` (Card, Badge, Button) |
| Calendar / RTH / holidays | `src-tauri/src/utils/market_calendar/` |

## Sequencing + cadence

- **Day 1 (Phase 1):** persistence layer ships. Backend-only. Visible win: from a dev tools console call `performance_capture_snapshot_now` → row in `tracker.sqlite`. Restart app → catch-up snapshot writes if today's row is missing. Wait one trading day → automatic snapshot.
- **Days 2–3 (Phase 2):** compute + read commands. Visible win: from a Claude Code session or the dev tools console, `performance_get_curve('all')` and `performance_get_metrics('all')` return populated structs (after seeding a few synthetic snapshots for dev).
- **Days 3–5 (Phase 3):** UI tab. Visible win: open the desktop app, click **Performance** in the sidebar — see the equity curve, metric cards, period selector. Empty state and 1-snapshot state render correctly.

Phase 1 → 2 → 3 is the strict critical path. No parallelism.

## Cross-phase verification

1. **Tracer-bullet (Phase 1 exit):** with `pnpm tauri dev` running and IBKR connected, click the dev-tools-bound `performance_capture_snapshot_now` → exactly one row appears in `account_snapshots` for today. Re-running within the same day → still one row, with updated `captured_at`. Restart the app the next trading day after 16:01 ET → automatic row written; `/tmp/qk-tauri.log` shows the snapshot capture log line.
2. **Tracer-bullet (Phase 2 exit):** seed `account_snapshots` with 5 synthetic rows spanning a peak-trough-recovery sequence; call `performance_get_metrics('all')` → response carries `total_return_pct ≈ +3.5%`, `max_drawdown_pct ≈ -1.6%`, `best_day_pct ≈ +3.7%`, `worst_day_pct ≈ -1.4%`, `days_tracked = 4`. Numbers cross-checked against a hand calc.
3. **Tracer-bullet (Phase 3 exit):** open the desktop app on a fresh install (no snapshots yet) → tab shows the empty state + "Capture snapshot now" button; click it → tab flips to the 1-snapshot state. Seed three more synthetic rows directly in SQLite → reload → chart renders, period selector switches `1W/1M/3M/YTD/1Y/All` cleanly, MetricsCards populate.
4. **CI invariant — surveillance-only:** the existing surveillance test (which greps MCP tool sources for `place_order` etc.) covers this program implicitly because no Performance MCP tool ships in v1. If a future phase adds one, the test must re-pass with the new file included.
5. **CI invariant — schema forward-compat:** a unit test inserts one row with `source = 'live_snapshot'` and one with `source = 'flex_query'` into `account_snapshots`, and asserts `get_curve` reads both transparently.
6. **CI invariant — idempotent UPSERT:** unit test inserts twice for the same `(account, date)` and asserts a single row.
7. **CI invariant — XIRR correctness:** `irr.rs` unit tests pin three known-answer cases (single deposit + terminal value; intra-period flow; negative-IRR case) to ±1e-7 against Excel/Sheets reference values.
8. **CI invariant — TWR with zero cash flows == period total return:** assert that for any non-empty snapshot slice, `twr(snapshots, &[]) == V_end / V_start − 1`.

## Open risks

- **Forward-only history.** The curve is empty until the first snapshot is captured, and meaningful only after several days. The UI footer caption says "tracking since YYYY-MM-DD; N trading days" so the user understands the window. — owned by Phase 1 (semantics) and Phase 3 (UI copy).
- **No cash-flow data.** Without deposit/withdrawal info, MWR collapses to CAGR. Honest labeling (`Annualized Return (CAGR)`) keeps the UI accurate. The `cash_flows` table exists from day 1, so a future v2 (manual entry UI, or FlexQuery import) is purely additive. — owned by Phase 2 (compute defaults) and Phase 3 (UI copy).
- **Snapshot timing skew.** IBKR resets daily P&L around 16:05 ET; if the EOD tick fires after 16:05, the captured `NetLiquidation` already reflects the next session's overnight base. Mitigation: tick at 16:01 ET, before reset. If TWS is not connected at tick time, the tick logs and skips — startup catch-up the next session covers it. — owned by Phase 1.
- **Multi-account aggregation.** v1 stores one row per account per day; the UI in v1 shows the aggregate (sum of NetLiq across accounts, sum-weighted returns) for simplicity. A future "switch account" dropdown is purely additive. — owned by Phase 3.
- **Mixed currency.** v1 assumes one currency. If a user has accounts in multiple currencies, the UI shows a warning banner and metrics are hidden until v2. — owned by Phase 3.
- **`recharts` bundle size.** ~95 KB gzipped added to the Vite build. Acceptable for a desktop app; revisit only if perf budget tightens. — owned by Phase 3.

## Out of scope

- **FlexQuery backfill.** Schema is shaped for it; integration is a separate v2 program.
- **Manual cash-flow entry UI.** v2.
- **Intraday live tick** of the curve. v2 (would subscribe to `start_daily_pnl_stream`).
- **MCP read tools** (`get_equity_curve`, `get_performance_metrics` for external agents). v2.
- **Per-symbol P&L attribution** (the `executions` table already exists from V13; attribution is a separate program).
- **Multi-currency conversion / consolidation.** v2 if ever.
- **Order placement.** Forever out of scope (surveillance-only invariant).
