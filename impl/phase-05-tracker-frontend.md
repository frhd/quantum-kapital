# Phase 05 — Tracker frontend

## Goal

A working Tracker tab where the user can see, add, tag, and remove watchlist symbols, plus an "Add to tracker" action on every Scanner result row.

## Depends on

- [x] Phase 04 — backend commands available.

## Out of scope

- Setups, alerts, MorningPack — the panels for those land in Phase 15 / 20 / 21.
- Drag-and-drop reordering, multi-select, bulk edit.

## Test plan (write tests FIRST)

There is no React unit test framework in this project (per `CLAUDE.md`: "frontend changes are verified manually"). Use **manual end-to-end checklists** as the test plan; tick when reproduced.

- [x] **E2E:** Open Tracker tab on a fresh DB → empty state with "no tickers tracked" message renders.
- [x] **E2E:** Click "Add" → dialog opens with symbol input, tag chips (Breakout / Episodic Pivot / Parabolic Short / +Custom), source defaulted to `Manual`, notes textarea.
- [x] **E2E:** Submit dialog with `AAPL` + `Breakout` tag → row appears immediately in Watchlist; counts in tab badge update.
- [x] **E2E:** Click row's "Open in analysis" → routes to existing `TickerAnalysis` with `pendingSymbol={symbol: 'AAPL', nonce: <fresh>}`.
- [x] **E2E:** Click row's "Edit tags" → tag chips populate with existing selection; can toggle, save persists.
- [x] **E2E:** Click row's "Remove" → row gone, no leftover state.
- [x] **E2E:** Refresh app (hard reload) → Watchlist re-fetches via `tracker_list`, all data intact.
- [x] **E2E:** From Scanner, click "Add to tracker" on a row → dialog opens pre-filled with `symbol`, `source: 'scanner'`, `source_meta: { rank, scan_code, exchange, contract_id }`. Submit → row appears in Tracker tab. Re-attempt on same symbol → toast "already tracked", no duplicate row.
- [x] **Status filter:** filter dropdown (All / Watching / In Play / Setup Active / Cool Down) filters the table client-side.
- [x] **A11y check:** Tab key navigates dialog inputs; ESC closes; focus returns to triggering button.

## Implementation tasks

- [x] Create `src/features/tracker/types.ts` mirroring backend types (`TrackedTicker`, `TrackerSource`, `TrackerStatus`, `StrategyTag`). Use string-union types matching backend snake_case.
- [x] Add to `src/shared/api/ibkr.ts` a `tracker` namespace with `add`, `remove`, `list`, `get`, `setTags`, `setStatus`, `fetchBars`, `getNews` (the last two from Phase 02/03, register here too if not already).
- [x] Create `src/features/tracker/hooks/useWatchlist.ts` — `{ tickers, loading, error, add, remove, setTags, refresh }`. Refresh on mount + after every mutation.
- [x] Create `src/features/tracker/components/Watchlist.tsx` — `Table` from `shared/components/ui/table.tsx`, columns: Symbol, Tags, Source, Status, Added (relative time), Actions. Loading uses `Skeleton`. Errors use `Alert`.
- [x] Create `src/features/tracker/components/AddToTrackerDialog.tsx` — controlled dialog using shadcn-style primitives (existing `Card`/`Input`/`Label`/`Button`). Tag chips are toggle-buttons. Custom tag input.
- [x] Create `src/features/tracker/components/TrackerTab.tsx` — composes Watchlist + Add button + AddToTrackerDialog + status filter dropdown. Handles `onSelectSymbol` callback (passed up to `App.tsx` for analysis deep-link).
- [x] Modify `src/features/scanner/components/ScannerResults.tsx`:
  - Existing single click stays as the "open in analysis" affordance. Replace the row click with two explicit buttons in an Actions column: "Analyze" (existing behavior) and "Add to tracker" (opens `AddToTrackerDialog` with pre-filled props).
- [x] Modify `src/app/App.tsx` — add a Tracker tab between Scanner and Analysis. Lift `pendingSymbol` state to handle deep-links from both Scanner and Tracker rows.

## Verification

- [x] `pnpm build` — no TS errors.
- [x] Run through every E2E checklist item above in `pnpm tauri dev` against a real IBKR + AV setup.
- [x] Visual check: theming matches existing tabs (Tailwind + lucide icons), no layout overflow at common viewport widths.

## Files

**Created:**
- `src/features/tracker/types.ts`
- `src/features/tracker/hooks/useWatchlist.ts`
- `src/features/tracker/components/Watchlist.tsx`
- `src/features/tracker/components/AddToTrackerDialog.tsx`
- `src/features/tracker/components/TrackerTab.tsx`

**Modified:**
- `src/shared/api/ibkr.ts`
- `src/features/scanner/components/ScannerResults.tsx`
- `src/app/App.tsx`

## Scratchpad

None for this phase.

## Done when

All E2E checklist items pass; Tracker tab is the user-visible entry point for watchlist management; scanner "Add to tracker" works without duplicate-row regressions; existing scanner → analysis deep-link still works.
