# Unified Ticker Workspace: page-centric → symbol-centric navigation

## Context

Today the sidebar is a list of pages and per-ticker context is just one of them. Click a ticker in the Watchlist or Scanner and `App.tsx` sets `pendingAnalysisSymbol` and routes to the **Analysis** page (`src/app/App.tsx:37`); the Analysis page then renders fundamentals + projection + sentiment for that symbol (`src/features/analysis/components/TickerAnalysis.tsx:18`). But every other thing keyed by symbol — research notes (already filterable: `src/features/research/hooks/useResearchNotes.ts:9`), alerts, news, predictions/outcomes, watchlist tags/setups — lives in a different sidebar tab with its own filter input. To assemble a full view of one symbol the user has to traverse five tabs and re-type the same symbol into each.

**Inversion.** The unit of work is the ticker, not the page. End state: a single **Workspace** page where the active symbol is shared state and tabs render per-symbol slices (Overview / Projection / News / Research / Alerts / History / Watchlist-meta). The sidebar's per-symbol entry collapses into the workspace. The cross-symbol entries (Tracker, Scanner, Candidates, Research, Eval) survive as **navigators** — global lists whose primary action is to set the active symbol and route into the workspace, optionally with a default tab.

Scope is frontend + a couple of small backend filter additions (per-symbol alerts/news/outcomes commands). No new LLM loops, no order surfaces, no schema changes.

## End-state architecture

| Subsystem | Responsibility |
|---|---|
| **`WorkspaceContext`** (new, `src/features/workspace/context/`) | Single source of truth for the active symbol, the active tab, and recent symbols. Provider mounted above the page switch in `App.tsx`. |
| **`useTickerNavigate(symbol, tab?)`** (new) | The only sanctioned way to set active symbol + tab from anywhere in the app. Replaces today's `handleSelectSymbol` prop drilling. |
| **`WorkspaceTab`** (new, `src/features/workspace/components/`) | Page-level component. Renders search + symbol summary + tab nav + the **active** panel only (lazy mount). |
| **Tab panels** (new) | `OverviewPanel`, `ProjectionPanel`, `NewsPanel`, `ResearchPanel`, `AlertsPanel`, `HistoryPanel`, `WatchlistMetaPanel`. Each owns its own data hooks; reads symbol from context. |
| **Existing list views** (Watchlist, AlertFeed, MorningPack, ResearchTab list, CandidateBrowser, MarketScanner) | Continue to render their own data; click handler swaps from per-feature `onSelectSymbol` props to `useTickerNavigate(...)`. |
| **`shared/api/*.ts`** | Still the only place that calls `invoke()`. Per-symbol filter additions in this arc go through these wrappers. |
| **Tauri commands touched** | `tracker_list_alerts` (add `symbol` filter), `get_news` (per-symbol command), `get_outcomes`/`get_prediction_history` (already symbol-scoped per MCP — expose to UI). No new write commands. |

## Hard invariants

1. **Surveillance-only stays.** The workspace exposes no order-placement surface. CI greps `features/workspace/**` for forbidden symbols (e.g. `place_order`, `placeOrder`).
2. **Single source of truth for active symbol.** Active symbol lives in `WorkspaceContext`. No parallel state in panels; no prop drilling between sibling panels.
3. **Lazy panel mount.** Inactive tabs do not call `invoke()`. Only the active panel's hooks fire. Switching tabs is what triggers a panel's data fetch.
4. **Tauri access only via `shared/api/*.ts`.** Workspace files MUST NOT call `invoke()` directly. Vitest greps the source.
5. **Reuse before invent.** No new business logic in the workspace — it composes existing hooks (`useResearchNotes`, `useAlerts`, `useProjections`, `useQuote`, `useTickerSearch`, `useEvalDashboard`) and existing components (`TickerCards`, `ProjectionView`, `SentimentWidget`, `AlertRow`, `TagEditor`, `SetupBadge`, `AddToTrackerDialog`).
6. **No regressions on existing entry points.** Scanner / Watchlist / pendingSymbol flows still land on a working symbol view at every phase boundary, even before all panels are wired.
7. **Pre-commit sacred** — `cargo fmt --check`, `cargo clippy -D warnings`, `prettier --check`, `eslint`. Never `--no-verify`.
8. **Frontend file caps.** TS/TSX soft 200, hard 350 (`CONTRIBUTING.md`). The workspace shell stays small; each panel lives in its own file.
9. **Mock-friendly seams unchanged.** Any backend filter added in this arc keeps the `IbkrClientTrait` seam intact.

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **State container:** React Context (`WorkspaceProvider`). No new state library.
- **Tab order:** Overview · Projection · News · Research · Alerts · History · Watchlist. Subject to UX iteration in Phase 1.
- **Default tab on entry:** Overview unless caller passes `tab` to `useTickerNavigate`.
- **Default-tab map by entry point:** alert click → Alerts · research note card → Research · everything else → Overview.
- **Sidebar id:** rename `analysis` → `ticker` (label "Ticker"). `currentPage` is in-memory, no migration cost.
- **Recents persistence:** `localStorage` key `qk:workspace:recent`, capacity 10. Active symbol NOT persisted across reloads (fresh start).
- **Per-symbol alerts filter:** backend (`tracker_list_alerts(symbol=...)`). Cleaner than client-side; pagination stays correct. (Phase 2.)
- **Per-symbol news filter:** backend command per symbol; reads existing `news_cache`. (Phase 3.)
- **Per-symbol outcomes filter:** extend `useEvalDashboard` to accept optional `symbol`, or extract a sibling hook. (Phase 3.)

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. Workspace shell + Overview tab (cutover from Analysis page) | [phase-1-shell-overview.md](phase-1-shell-overview.md) | — | done (commit 466a670, 2026-05-02) |
| 2. Research + Alerts + Watchlist-meta tabs | [phase-2-research-alerts-meta.md](phase-2-research-alerts-meta.md) | 1 | done (commit 642350a, 2026-05-02) |
| 3. News + History tabs | [phase-3-news-history.md](phase-3-news-history.md) | 1 | done (commit 917202c, 2026-05-03) |
| 4. Universal navigation (every entry point routes to workspace) | [phase-4-universal-nav.md](phase-4-universal-nav.md) | 2, 3 | done (commit 2686146, 2026-05-03) |
| 5. Recent symbols + quick switcher (Cmd+K) | [phase-5-recents-switcher.md](phase-5-recents-switcher.md) | 4 | in-progress (started 2026-05-03) |

> **Status convention:** `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| App-level routing + page state | `src/app/App.tsx` |
| Sidebar nav + `PageId` enum | `src/shared/components/layout/Sidebar.tsx` |
| Layout shell | `src/shared/components/layout/AppLayout.tsx` |
| Existing analysis page (becomes Overview tab) | `src/features/analysis/components/TickerAnalysis.tsx` |
| Ticker cards / projection / search / sentiment | `src/features/analysis/components/{TickerCards,ProjectionView,TickerSearch}.tsx`, `src/features/sentiment/components/SentimentWidget.tsx` |
| Reused symbol-keyed hooks | `src/features/analysis/hooks/{useTickerSearch,useProjections,useQuote}.ts`, `src/features/research/hooks/useResearchNotes.ts`, `src/features/tracker/hooks/{useAlerts,useWatchlist,useTrackerEvents}.ts`, `src/features/eval/hooks/useEvalDashboard.ts` |
| List views that become navigators | `src/features/tracker/components/{Watchlist,AlertRow,MorningPack}.tsx`, `src/features/research/components/ResearchTab.tsx`, `src/features/candidates/components/CandidateBrowser.tsx`, `src/features/scanner/components/MarketScanner.tsx` |
| Tauri command wrappers (only place `invoke()` is called) | `src/shared/api/ibkr.ts` |
| Backend tracker alerts command (Phase 2 filter add) | `src-tauri/src/ibkr/commands/tracker.rs` (`tracker_list_alerts`) |
| Backend per-symbol news + outcomes commands (Phase 3) | `src-tauri/src/ibkr/commands/` (commands TBD per existing surface) |
| MCP tool reference for symbol-keyed reads (parity guide) | `src-tauri/src/mcp/tools/{news,outcomes,prediction_history,fundamentals}.rs` |
| Frontend rules (file caps, `shared/api/` only) | `src/CLAUDE.md`, `CONTRIBUTING.md` |
| Vitest setup | `src/test/setup.ts` |

## Sequencing + cadence

- **W1:** Phase 1 — workspace shell + Overview migration. Gates everything else.
- **W2:** Phase 2 — Research + Alerts + Watchlist-meta. Highest-value tabs (deepest user complaint).
- **W3:** Phase 3 — News + History. Can run parallel with Phase 2 on a separate branch since it touches different panels.
- **W4:** Phase 4 — wire every list to `useTickerNavigate` and demote ResearchTab/Eval to global browsers.
- **W5:** Phase 5 — recents + Cmd+K switcher.

Phase 4 is the integration phase: it turns five disjoint click handlers into one. Phase 5 is polish; ship even if Phase 5 slips.

## Cross-phase verification

1. **Tracer-bullet (Phase 1 exit):** Click a ticker in Scanner → workspace opens → Overview tab renders the same fundamentals/quote/projection/sentiment payload the old Analysis page produced. Vitest covers `useTickerNavigate` setting symbol context + page; manual smoke covers the visual parity.
2. **Tracer-bullet (Phase 2 exit):** Click a tracked watchlist row → workspace opens → switching to Research tab shows that symbol's notes, switching to Alerts tab shows that symbol's alerts (paged correctly), switching to Watchlist-meta shows tags/setup. An untracked symbol shows the "Add to tracker" CTA in Watchlist-meta.
3. **Tracer-bullet (Phase 4 exit):** From every list (Watchlist, AlertFeed, MorningPack, ResearchTab note card, CandidateBrowser row, Scanner result), clicking a ticker calls `useTickerNavigate` and lands on the correct default tab. Vitest fixture per entry point.
4. **CI invariant — `invoke()` containment:** Vitest greps `src/features/workspace/**/*.{ts,tsx}` for `from "@tauri-apps/api/core"` and bare `invoke(` calls; expects zero hits.
5. **CI invariant — surveillance-only:** Vitest greps `src/features/workspace/**` for `place_order|placeOrder|orderRef|new Order`; expects zero hits.
6. **CI invariant — single-source-of-truth:** Vitest asserts panels do not accept a `symbol` prop (panels read from context only).
7. **File-size check (per phase):** every new `*.tsx` under `features/workspace/` ≤ 350 LOC; the workspace shell ≤ 200 LOC.
8. **No regressions:** existing vitest suites for analysis/scanner/tracker continue to pass after every phase boundary.

## Open risks

- **Tab fetch fan-out.** Mounting all panels eagerly fires N hooks on each symbol switch. Mitigation: lazy-mount the active panel; unmount on switch. Hard invariant 3 enforces this.
- **Deep-link tab semantics drift.** Each entry point wants a different default tab. Mitigation: agreed default-tab map (above); `useTickerNavigate(symbol, tab?)` is the only navigation primitive, so the choice is explicit at every call site.
- **Empty-state proliferation.** Each tab has loading/error/empty branches; easy to drift into seven different empty UIs. Mitigation: a single `<EmptyState />` component introduced in Phase 2 and reused; Phase 3 panels MUST consume it.
- **Alert symbol-filter perf.** `useAlerts` paginates; client-side symbol filter sees only the current page and could miss older alerts. Mitigation: backend filter in `tracker_list_alerts` (default chosen above).
- **Coordination with AV strip-out plan.** News tab in Phase 3 reads `news_cache` SQLite, which is producer-agnostic. AV→IBKR provider migration in `loop/plan/` is invisible to the workspace. Phase 3 must NOT call any AV adapter directly.
- **Research/Eval demotion.** Removing the standalone sidebar entries is a UX bet. Mitigation: keep them as global cross-symbol browsers in Phase 4; their note cards/rows additionally route into the workspace.
- **Sidebar rename.** "Analysis" → "Ticker" forces small habit change. Acceptable for a single-user desktop app.
- **Context re-render cascades.** Splitting context (symbol-only vs. tab-only vs. recents-only) may be needed if perf bites. Phase 5 reassesses.
- **HMR loses active symbol.** Acceptable; Phase 5 can back recents (but not active symbol) with `localStorage`.
- **MorningPack ticker tokens are not currently links.** Phase 4 adds click handlers; verify the layout doesn't break.
