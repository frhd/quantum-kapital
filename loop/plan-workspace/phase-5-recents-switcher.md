# Phase 5 — Recent symbols + quick switcher (Cmd+K)

> Part of [Unified Ticker Workspace](master.md). See index for invariants.

**Status:** todo

**Depends on:** 4

**Goal:** Make symbol switching frictionless. A recent-symbols list (localStorage-backed, capped at 10) lives in the workspace empty state and the sidebar. A Cmd/Ctrl+K palette opens from anywhere and fuzzy-matches across cached tickers + watchlist + recents, routing into the workspace on selection. This phase is polish — the workspace is functionally complete after Phase 4.

## Files

- New: `src/features/workspace/hooks/useRecentSymbols.ts` — localStorage-backed list, dedup-on-add, capacity 10. API: `{ recents: string[], push(symbol), clear() }`.
- New: `src/features/workspace/components/RecentSymbolChips.tsx` — chip strip; click chip → `navigate(symbol)`.
- New: `src/features/workspace/components/QuickSwitcher.tsx` — modal palette. Cmd/Ctrl+K to open; arrow keys + enter to navigate.
- Touches: `src/features/workspace/context/WorkspaceContext.tsx` — wire `push(symbol)` into the `navigate` call so every navigation feeds the recent list. Persist on change.
- Touches: `src/features/workspace/components/WorkspaceTab.tsx` — empty state shows `RecentSymbolChips`; a new "Recent" section.
- Touches: `src/app/App.tsx` (or `WorkspaceProvider` mount site) — mount `<QuickSwitcher />` once globally so the keyboard shortcut works on any page.
- New: tests — `useRecentSymbols.test.ts` (dedup, cap, persistence), `QuickSwitcher.test.tsx` (open/close, arrow nav, selection routes).

## Reuse (no new business logic this phase)

- `WorkspaceContext` + `useTickerNavigate` from Phase 1.
- `useTickerSearch.cachedTickers` (or its underlying `getCachedTickers()` Tauri call) for the universe of selectable symbols.
- `useWatchlist` for tracked symbols.
- shadcn-style `<Dialog>` / `<Command>` primitives in `src/shared/components/ui/` if present; otherwise compose with raw modal + input.

## Decisions to make in this phase

- **Persistence scope.** Recommended: persist **recents** in localStorage; do NOT persist the active symbol or tab across reloads. Active symbol resets each session — feels right for a desk app and avoids surprise on cold start.
- **Switcher data sources.** Recommended: `recents ∪ watchlist ∪ cachedTickers` deduped, recents shown first. No remote search by name in v1; the cached list is large enough.
- **Keyboard shortcut.** Cmd+K (macOS) / Ctrl+K (Linux/Windows). Listen at the provider level; preventDefault.
- **Sidebar recents strip.** Optional UX nicety — recent chips below the sidebar nav. Recommended **defer** to a follow-up; the empty-state recents + Cmd+K cover the use case.
- **Context split for perf.** If recents updates re-render the entire tree (because they share a context with `symbol`/`tab`), split into a sibling context. Recommended: defer unless profiling shows a problem.

## Exit criteria

- `pnpm typecheck && pnpm lint && pnpm test:run` green.
- Manual: Cmd+K opens the palette; typing narrows; arrow keys move; enter navigates and the workspace opens to Overview.
- Manual: the workspace empty state shows a "Recent" chip strip; clicking a chip navigates.
- Manual: visiting symbols updates the recents list; capped at 10; reload preserves recents.
- Vitest: `useRecentSymbols` dedup + cap + localStorage round-trip work.
- Vitest: `QuickSwitcher` opens on the keyboard shortcut, filters on input, arrow keys move selection, enter routes via `useTickerNavigate`.
- File-size + invariant greps continue to pass.

## Gotchas

- **localStorage in jsdom.** `src/test/setup.ts` already provides a localStorage shim — verify before adding dependencies.
- **Duplicate keys.** When a symbol is re-visited, move it to the front rather than push a duplicate. Capacity is enforced after the move.
- **Keyboard shortcut conflicts.** Tauri webviews on Linux don't override Cmd+K by default; verify on the actual desktop runtime, not just jsdom.
- **Modal focus trap.** The palette must trap focus and restore it on close. Use the existing dialog primitive; don't roll your own.
- **Cached tickers count.** `getCachedTickers()` may return many hundreds; render filtered results virtualized or hard-cap displayed rows (e.g. top 50) to keep the palette responsive.
- **Recents drift across plan branches.** If a future plan introduces another navigation primitive (e.g. opening a non-symbol "view"), the recents list must remain symbol-only. Type the localStorage payload strictly.
- **Active-symbol persistence (deliberately rejected).** If user pushback emerges, revisit — but defaulting to "fresh on cold start" keeps state simple and reduces stale-data risk if the user comes back the next morning.
