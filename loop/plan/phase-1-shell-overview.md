# Phase 1 — Workspace shell + Overview tab (cutover from Analysis page)

> Part of [Unified Ticker Workspace](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-02)

**Depends on:** none (foundation phase)

**Goal:** Stand up the workspace skeleton — context provider, tab nav, lazy panel mount, the `useTickerNavigate` primitive — and migrate the existing `TickerAnalysis` page contents into the **Overview** tab without losing parity. After this phase, every existing entry point (Scanner `onSelectSymbol`, Watchlist row click, `pendingAnalysisSymbol`) lands on the workspace's Overview tab and renders identical data to the old page. Other tabs render placeholder panels stating "Coming next phase," so the tab nav is stable from day one.

## Files

- New: `src/features/workspace/context/WorkspaceContext.tsx` — provider + `useWorkspace()` hook. State shape: `{ symbol: string | null, tab: WorkspaceTabId, setSymbol, setTab, navigate(symbol, tab?) }`.
- New: `src/features/workspace/hooks/useTickerNavigate.ts` — thin wrapper that pulls `navigate` from context plus calls `onNavigate("ticker")` on the page router. Increments an internal nonce on every call so re-clicking the same symbol still re-runs symbol-side fetches.
- New: `src/features/workspace/types.ts` — `WorkspaceTabId = "overview" | "projection" | "news" | "research" | "alerts" | "history" | "watchlist"`.
- New: `src/features/workspace/components/WorkspaceTab.tsx` — page-level container. Renders header + tab nav + the active panel only.
- New: `src/features/workspace/components/WorkspaceHeader.tsx` — search input + symbol summary banner. Reuses `TickerSearch`.
- New: `src/features/workspace/components/WorkspaceTabsNav.tsx` — tab buttons; reads/writes context.
- New: `src/features/workspace/components/panels/OverviewPanel.tsx` — wraps `TickerCards` + `ProjectionView` + `SentimentWidget`. Reads symbol from context.
- New: `src/features/workspace/components/panels/PlaceholderPanel.tsx` — stub for not-yet-implemented tabs ("Coming in Phase N").
- New: `src/features/workspace/components/WorkspaceTab.test.tsx` — vitest: empty state when no symbol, Overview renders on `useTickerNavigate("AAPL")`, tab switch unmounts inactive panels (assert via spy on placeholder render).
- New: `src/features/workspace/hooks/useTickerNavigate.test.ts` — vitest: setting symbol updates context; nonce increments on repeat-call; tab override accepted.
- Touches: `src/app/App.tsx` — wrap content with `<WorkspaceProvider>`; replace `<TickerAnalysis pendingSymbol=... />` with `<WorkspaceTab />`; delete `pendingAnalysisSymbol` state and `handleSelectSymbol` prop drilling at this site (entry-point components keep their existing `onSelectSymbol` props until Phase 4 — App.tsx now binds them to `useTickerNavigate`).
- Touches: `src/shared/components/layout/Sidebar.tsx` — rename `PageId` value `"analysis"` → `"ticker"`, label "Ticker". Update icon if desired (`LineChart` is fine).
- Touches: `src/features/analysis/components/TickerAnalysis.tsx` — delete (no remaining callers after `App.tsx` swap).
- Touches: `src/features/analysis/components/TickerSearch.tsx` — small adjustment if needed so the dropdown renders correctly inside `WorkspaceHeader` (z-index preserved).

## Reuse (no new business logic this phase)

- `useTickerSearch` (`features/analysis/hooks/useTickerSearch.ts`) — drives search dropdown.
- `useProjections`, `useQuote` — feed Overview content unchanged.
- `TickerCards`, `ProjectionView`, `TickerSearch`, `SentimentWidget` — composed inside `OverviewPanel` / `WorkspaceHeader`.
- Existing `IbkrClientTrait`-backed Tauri wrappers — no new wrappers this phase.

## Decisions to make in this phase

- **Sidebar id rename `analysis` → `ticker`.** Recommended yes — clarifies intent and the in-memory enum has no migration cost. Rejecting means leaving a stale name in code.
- **Active symbol in context vs. hoisted props.** Recommended **context** — Phase 4 wires entry points app-wide; prop drilling does not scale.
- **Render only active panel vs. all panels.** Recommended **only active** — enforces hard invariant 3 (lazy fetch).
- **Tab placeholders for unimplemented panels.** Recommended **show placeholders** — keeps the tab nav visually stable and signals next-phase scope.
- **Where `useTickerSearch` lives.** Leave in `features/analysis/hooks/` for now (cross-feature import); revisit consolidation in Phase 4.
- **Connection guard.** Workspace requires `connectionStatus.connected` like every other page; surface "connect first" empty state inside `WorkspaceTab` (or keep guard at `App.tsx` — recommended: keep at `App.tsx` consistent with siblings).

## Exit criteria

- `pnpm typecheck`, `pnpm lint`, `pnpm test:run` all green.
- Manual: connect to TWS → Sidebar shows "Ticker" → click empty state → search "AAPL" → Overview tab renders fundamentals + quote + projection + sentiment exactly as the old Analysis page did.
- Manual: from Scanner, click a result → workspace opens to Overview with that symbol; from Tracker watchlist, click a row → same behavior.
- Manual: click the same symbol twice — fundamentals/quote re-fetch (nonce semantics preserved).
- Vitest: `WorkspaceTab` empty state renders when context has `symbol === null`; switching tabs unmounts the previous panel (use a spy/mock on `OverviewPanel` mount/unmount lifecycle).
- Vitest: `useTickerNavigate("AAPL", "overview")` sets context symbol="AAPL", tab="overview", and triggers page change to `"ticker"`.
- File-size: `WorkspaceTab.tsx` ≤ 200 LOC; every other new file ≤ 350 LOC.
- Zero `invoke(` references inside `src/features/workspace/**` (verified via vitest grep — adds the test that future phases reuse).
- `pendingAnalysisSymbol` state and `handleSelectSymbol` are gone from `App.tsx`.

## Gotchas

- **Nonce preservation.** The current `pendingAnalysisSymbol` uses a nonce so clicking the same symbol again re-fires `selectTicker`. `useTickerNavigate` MUST replicate this — store a counter in the context and bump it every call; the Overview panel keys an effect on it to re-fetch. Without the nonce, the second click is a no-op and stale data lingers.
- **TickerSearch z-index.** The dropdown today relies on the wrapping Card's `z-50` and `overflow-visible` (`TickerAnalysis.tsx:48-49`). Carry these classes into `WorkspaceHeader` or the dropdown will be clipped.
- **`useTickerSearch` initial-load fetch.** It calls `getCachedTickers()` on mount (`useTickerSearch.ts:14-32`). Mount it once at the workspace level — not inside every panel — to avoid a fetch storm.
- **`PageId` widening.** Renaming `analysis` → `ticker` updates `PageId`, `NAV_GROUPS`, `PAGE_LABELS`, and every consumer that switches on it. Run `tsc` early to find them.
- **Empty-state drift.** Define a small `<EmptyState />` here (or defer to Phase 2) so `OverviewPanel` and `PlaceholderPanel` use the same primitive. Recommended: defer the shared component to Phase 2 when there are real consumers; for now the placeholder can be inline.
- **HMR + context.** During hot reload the provider remounts and the active symbol resets to `null`. Acceptable for this phase; Phase 5 may add `sessionStorage` backing.
- **`AddToTrackerDialog` mount site.** Stays at `App.tsx` (it's a global modal). Workspace panels open it via the existing `setAddDialogOpen` prop chain; Phase 2 can introduce a small store/context if the chain gets long.
- **Connection-disconnected behavior.** When TWS disconnects mid-session the workspace should not crash on `null` quote/projection responses. The reused hooks already handle this — verify with a fixture test.
