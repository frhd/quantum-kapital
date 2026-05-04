# Phase 3 — Frontend "Today's Trades" panel + Tauri command

> Part of [Trade history visibility](master.md). See index for invariants.

**Status:** done (commit dde1a8f, 2026-05-04)

**Depends on:** 2

**Goal:** A new `Trades` tab in the desktop app shows today's IBKR fills,
grouped by symbol (and by option-key for option fills), with a summary
banner (gross realized P&L, total fees, net P&L, fill count). The same
data the MCP tool returns, but presented in the UI for at-a-glance
end-of-day review. Date picker is wired so the page is forward-compatible
with Phase 4 (persistence) — for now, prior dates simply render an
empty state.

## Files

- New: `src/features/trades/` — feature folder. Files within
  (mirroring the layouts in `src/features/portfolio/` and
  `src/features/candidates/`):
  - `TradesPage.tsx` — top-level page component. Date picker, summary
    banner, grouped list, empty state.
  - `TradesGroup.tsx` — collapsible per-symbol (and per-option-key)
    group. Header shows group label, leg count, gross P&L, fees, net
    P&L. Body renders `TradesLeg` rows when expanded.
  - `TradesLeg.tsx` — single fill row: time (ET), side, qty, avg
    price, commission, realized P&L. Use `—` placeholder for any
    `null` field.
  - `useTrades.ts` — React Query hook that wraps the Tauri command.
    `staleTime: 0`, `refetchOnWindowFocus: true`. No polling timer.
  - `groupExecutions.ts` — pure function: `(rows: ExecutionRow[]) =>
    TradeGroup[]`. Two-level grouping: top by `symbol`, second by
    option-key tuple (`expiry`/`strike`/`right`/`multiplier`) when
    `contract_type === "OPT"`. Computes per-group totals.
  - `types.ts` — TypeScript mirror of the Rust `ExecutionRow` DTO.
    Single source of truth on the FE side.
  - `__tests__/groupExecutions.test.ts` — fixture-driven unit test
    for the grouping/aggregation function.
  - `__tests__/TradesGroup.test.tsx` — component test renders a
    fixture group, asserts header totals + per-leg rendering.
- New: `src-tauri/src/ibkr/commands/trades.rs` (or extend
  `commands/trading.rs` if file-size cap permits — a 50-LOC addition
  is fine in either) — Tauri command:
  ```
  #[tauri::command]
  pub async fn get_executions_for_date(
      state: tauri::State<'_, AppState>,
      account: Option<String>,
      date: String, // ISO YYYY-MM-DD; defaults to today on the FE side
  ) -> Result<Vec<ExecutionRow>, String>;
  ```
  Body resolves the `AccountReader` from app state, resolves the
  account (sole managed → default; multi → require arg), parses the
  ISO date, calls `account_reader.executions(account, date)`, returns
  the rows. Errors are stringified at this boundary (existing Tauri
  command pattern).
- Touches: `src-tauri/src/lib.rs` — register
  `get_executions_for_date` in the `tauri::generate_handler![...]`
  invoke list.
- Touches: the FE route registry / sidebar config (file location per
  `src/CLAUDE.md`) — add a `Trades` entry pointing to `/trades` and
  rendering `TradesPage`.

## Reuse

- React Query setup pattern from `src/features/portfolio/` or
  `src/features/candidates/`.
- The shadcn (or equivalent) `Collapsible` + `Card` primitives the app
  already uses for grouped lists. Check `src/components/ui/` for the
  vocabulary.
- The existing date-picker component (search `src/components/`); reuse
  rather than introduce a new one.
- The existing ET/UTC formatting utilities in `src/lib/` (search for
  `formatEt` / `etDate` patterns).
- Existing Tauri-`invoke` typed wrapper conventions per `src/CLAUDE.md`.
- The `AccountReader` seam (Phase 2) — the Tauri command does not
  re-implement any IBKR call; it routes through the same seam the MCP
  tool uses.

## Decisions to make in this phase

- **Route + tab name.** `/trades`, sidebar label "Trades". Mirrors the
  existing one-word route names.
- **Grouping precedence.** Top by `symbol`. Second tier by
  `(expiry, strike, right, multiplier)` only when `contract_type ===
  "OPT"`. Stocks never get a second tier. This keeps the user's
  mental model: TSLA appears once at the top with three sub-rows for
  the strike ladder.
- **Net P&L formula.** Per group, `net = sum(realized_pnl ?? 0) -
  sum(commission ?? 0)`. Surface gross and fees separately so the
  user sees how much the broker took.
- **Missing commission rendering.** Render the cell as `—` (em dash).
  Do not coalesce to `0` — that would silently mis-attribute fees.
  Group-level fee totals exclude `null` commissions and the header
  marks the group with a small badge "fees pending" when any leg has
  `commission === null`.
- **Empty state copy.** "No fills for {date}." With a small caption:
  "IBKR's executions endpoint only returns the current trading day.
  Prior days will populate once the executions store ships." Remove
  the caption when Phase 4 is `done`.
- **Auto-refresh.** React Query: `staleTime: 0`, `refetchOnWindowFocus:
  true`, no polling timer.
- **Multi-account handling.** Render an account picker in the page
  header **only when** the user has more than one managed account.
  Mirror whatever the Portfolio page does today; if Portfolio has no
  picker, ship without one in v1 and add when Portfolio adds it (the
  MCP tool already errors helpfully on multi-account-without-arg).
- **Time zone of displayed timestamps.** ET. Show as `HH:mm:ss`
  alongside the date in the page header. The DTO carries UTC, the
  conversion happens in `TradesLeg`.

## Exit criteria

- Manual: `pnpm tauri dev`, place at least one paper trade, open the
  Trades tab. Today's date is preselected. The fill appears in the
  correct symbol group with side / qty / avg price / commission / time
  rendered. Group header totals match the underlying rows.
- Manual: total of `gross - fees = net` matches the
  `mcp__quantum-kapital__get_executions` output sum to the cent.
- Vitest: `groupExecutions.test.ts` passes with fixtures covering
  (a) stock-only fills, (b) option-only fills with multiple strikes
  for the same symbol, (c) mixed stock + option for the same symbol,
  (d) commission `null` propagation into the group's "fees pending"
  flag.
- Vitest: `TradesGroup.test.tsx` renders a 3-leg fixture and asserts
  the header total + each leg row.
- TypeScript clean (`pnpm typecheck`).
- ESLint + prettier clean.
- File-size caps: every TSX file in `src/features/trades/` ≤ 200 LOC.
  If `TradesPage.tsx` approaches the cap, extract presentation
  fragments (e.g., `TradesSummaryBanner.tsx`).
- Pre-commit clean.

## Gotchas

- **Tauri-FE type sync.** The Rust `ExecutionRow` DTO and the TS
  mirror in `types.ts` must agree exactly. There is no codegen on
  this project (per `src/CLAUDE.md`); the manual mirror is the
  convention. Add a tiny round-trip test (Rust serialise → JSON →
  TS type) only if Phase 1 or Phase 2 didn't already do it for the
  MCP path.
- **`expiry: NaiveDate`** serializes as `"YYYY-MM-DD"` via serde
  default. The TS mirror types it `string` and parses to `Date` only
  at the formatting site.
- **`exec_time`** is UTC. The FE converts to ET for display. Don't
  let an old `new Date(exec_time)` pattern slip in without an
  explicit zone — the user is in ET and Linux defaults vary.
- **Group sort within the page.** Symbols ordered by the most recent
  fill in the group (descending by `max(exec_time)`). Inside a group,
  legs ordered ascending by `exec_time` so the user reads the day in
  sequence.
- **Long expiry-string formatting.** Option group headers like
  `TSLA 2026-05-04 $390 C` get long; truncate at the row level if
  they overflow but keep the full label in a tooltip.
- **Multi-currency rows.** USD-only in v1; if a non-USD currency
  appears in `currency`, surface it as a small tag on the leg row
  rather than silently treating it as USD. (Gracefully degrades for
  the rare future case.)
- **Routing churn.** When adding the new route, follow the exact
  pattern the existing `/portfolio` and `/scanner` use. Don't
  introduce a new routing primitive.
- **Realized P&L sign convention.** IBKR sends `realized_pnl` as
  positive on profitable closes, negative on losers. Display
  positive values in green, negative in red — mirror existing P&L
  cells elsewhere in the app to stay consistent.
- **Tauri command error stringification.** The command returns
  `Result<_, String>`. The FE useTrades hook's `onError` should map
  the string to a user-facing toast; don't render the raw string in
  the page body.
- **Window focus refetch + Tauri.** `refetchOnWindowFocus: true` is
  standard React Query but interacts with Tauri windows that lose
  focus on tray-collapse. Mirror whatever Portfolio does — if
  Portfolio uses `refetchOnMount` instead, do the same here.
