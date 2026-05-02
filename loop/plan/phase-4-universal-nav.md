# Phase 4 — Universal navigation: every entry point routes into the workspace

> Part of [Unified Ticker Workspace](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-03)

**Depends on:** 2, 3

**Goal:** Make the workspace the single landing zone for any per-symbol click in the app. Every list view — Watchlist, AlertFeed, MorningPack, ResearchTab note cards, CandidateBrowser rows, MarketScanner results, even research-note evidence ref chips — replaces its bespoke click handler with a `useTickerNavigate(symbol, tab?)` call. After this phase the workspace is the only per-symbol surface; lists are pure navigators with optional default-tab hints. ResearchTab and EvalTab survive as **global cross-symbol browsers** (their list views are still useful), but their items now route into the workspace too.

## Files

- Touches: `src/features/tracker/components/AlertRow.tsx` — make the row's symbol token clickable; on click, `navigate(alert.symbol, "alerts")`. Click target must not collide with existing ack/seen buttons.
- Touches: `src/features/tracker/components/Watchlist.tsx` — replace the `onSelectSymbol` prop call with `useTickerNavigate(symbol, "overview")`.
- Touches: `src/features/tracker/components/MorningPack.tsx` — wrap each ticker token in a navigator (default tab Overview).
- Touches: `src/features/research/components/ResearchTab.tsx` — `NoteCard` symbol header becomes a navigator → `(symbol, "research")`. Evidence-ref chips that point to alerts → `(alert_symbol, "alerts")`; chips that point to news → `(news_symbol, "news")`; chips that point to setups → `(setup_symbol, "watchlist")`. Schema dictates which fields exist; gate per `EvidenceRef.type`.
- Touches: `src/features/candidates/components/CandidateBrowser.tsx` — row click navigates with default Overview. (App.tsx currently does NOT wire a `onSelectSymbol` here — Phase 4 introduces it.)
- Touches: `src/features/scanner/components/MarketScanner.tsx` — replace `onSelectSymbol` prop with `useTickerNavigate(symbol, "overview")`.
- Touches: `src/app/App.tsx` — drop the `handleSelectSymbol` prop drilling; entry-point components no longer need `onSelectSymbol` props because they call the hook directly. `AddToTrackerDialog` mount stays at app-level.
- Touches: `src/features/eval/components/EvalTab.tsx` — symbol cells in prediction/outcome rows become navigators → `(symbol, "history")`.
- New: integration tests per entry point in `src/features/workspace/__tests__/navigation.test.tsx` — render each list with fixture data, click the symbol target, assert `useTickerNavigate` was called with the expected `(symbol, tab)`.
- New: vitest grep test asserting `features/workspace/**` does not import `@tauri-apps/api/core` (re-runs the Phase 1 invariant check; verifies it still holds).

## Reuse (no new business logic this phase)

- `useTickerNavigate` (Phase 1).
- `useWorkspace()` (Phase 1) — for any panel that needs the current symbol/tab.
- All existing list components, hooks, and data — only their click handlers change.
- ResearchTab + EvalTab + CandidateBrowser keep their global-browser roles.

## Decisions to make in this phase

- **ResearchTab demotion vs. retention.** Recommended **retention** as a global notes browser. The sidebar entry stays; clicking a note's symbol or evidence ref now routes into the workspace. Removing the tab entirely loses cross-symbol browsing, which is a real use case.
- **EvalTab demotion vs. retention.** Recommended **retention** for the same reason (cross-symbol accuracy is global).
- **Click target on `AlertRow`.** Recommended: only the symbol token is clickable; the rest of the row keeps existing ack/seen interactions. Avoids accidental navigations.
- **Default-tab map (locked).**
  - alert → Alerts
  - research note (header symbol) → Research
  - research note (evidence ref → alert) → Alerts
  - research note (evidence ref → news) → News
  - research note (evidence ref → setup) → Watchlist
  - watchlist row → Overview
  - morning pack item → Overview
  - candidate row → Overview
  - scanner result → Overview
  - eval prediction/outcome row → History
- **Removing `onSelectSymbol` prop.** Recommended **yes** — simpler than parallel paths. Components call the hook themselves.
- **Cross-feature import scrutiny.** `useTickerNavigate` is imported across many features. That's fine — it's the public navigation primitive owned by `features/workspace/`.

## Exit criteria

- `pnpm typecheck && pnpm lint && pnpm test:run` green; `cargo test` green.
- Vitest navigation suite: 7+ entry points each verified (Watchlist, AlertRow, MorningPack, ResearchTab note header, ResearchTab evidence ref, CandidateBrowser, MarketScanner, EvalTab).
- Manual: from each list, click any ticker → workspace opens with the correct default tab. No broken click handlers.
- Manual: `AddToTrackerDialog` still opens correctly from Scanner / Watchlist meta panel.
- `App.tsx` no longer holds `handleSelectSymbol`; the prop has been removed from entry-point components or replaced with internal hook usage.
- ResearchTab / EvalTab still function as global browsers (cross-symbol filter input still works).
- File-size + invariant greps continue to pass.

## Gotchas

- **Provider scope.** `WorkspaceProvider` must wrap the page switch in `App.tsx` so that lists rendered on other pages (Tracker, Research, Eval, Candidates, Scanner) can call `useTickerNavigate`. Mount it once at the top, around the entire content tree.
- **Click event bubbling on AlertRow.** Existing buttons inside the row handle their own clicks; the symbol-click handler must `stopPropagation` to avoid firing both. Cover with a vitest fixture.
- **Evidence ref schema.** `EvidenceRef` (`src/features/research/types.ts`) has variants for alert, news, setup, bar_range. The bar_range variant has no destination — leave it as a non-clickable chip.
- **MorningPack token layout.** Tickers may be inline within prose or in a structured list; converting them to clickable spans must preserve typography. Visual regression risk; manual smoke required.
- **CandidateBrowser was previously detached.** Adding navigation may surface other latent bugs in the candidate flow; keep the change minimal — just the row click — and file follow-ups in `QUESTIONS.md`.
- **Default-tab override callers.** A few sites (research evidence chips) need a different tab per item; pass it explicitly. Avoid burying the tab choice in the component's data shape — it should be obvious at the call site.
- **Removing prop drilling.** When `handleSelectSymbol` is deleted, hunt for stragglers — any leftover prop expecting it will be a typecheck error. Treat that as the canary, not a chore.
- **Tests for AlertRow click.** The existing AlertRow tests likely don't expect the new click handler; update them rather than adding a parallel suite.
