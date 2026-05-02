# Phase 2 — Research + Alerts + Watchlist-meta tabs

> Part of [Unified Ticker Workspace](master.md). See index for invariants.

**Status:** done (commit 642350a, 2026-05-02)

**Depends on:** 1

**Goal:** Stand up the three highest-value per-symbol panels — **Research**, **Alerts**, **Watchlist-meta** — inside the workspace shell. These are the ones the user most often re-types a symbol into today. Research reuses `useResearchNotes({ symbol })` (already filterable). Alerts gains a backend `symbol` filter on `tracker_list_alerts` so pagination stays correct. Watchlist-meta reuses `useWatchlist`, `TagEditor`, `SetupBadge` and exposes the existing `AddToTrackerDialog` for untracked symbols. After this phase, the workspace answers "what does the agent think of this ticker?" and "what's been firing on this ticker?" inline with the analysis view.

## Files

- New: `src/features/workspace/components/panels/ResearchPanel.tsx` — wraps `useResearchNotes({ symbol })`; renders note cards (extract `NoteCard` from `ResearchTab.tsx` into a shared component if not already extracted; otherwise import from there).
- New: `src/features/workspace/components/panels/AlertsPanel.tsx` — wraps the existing `useAlerts` hook with the new `symbol` arg; renders `AlertRow`s and reuses kind/onlyUnseen filters from the global feed.
- New: `src/features/workspace/components/panels/WatchlistMetaPanel.tsx` — pulls the watchlist row for the active symbol; renders `TagEditor`, `SetupBadge`, journal/notes summary if present, and an "Add to tracker" CTA when no row exists.
- New: `src/features/workspace/components/EmptyState.tsx` — shared empty-state component: title, description, optional CTA button.
- New: tests for each panel (`*.test.tsx`): symbol-scoped data renders, empty state when no data, error state, panel reads symbol from context (does NOT accept it as a prop).
- Touches: `src/features/tracker/hooks/useAlerts.ts` — extend `UseAlertsArgs` with `symbol?: string | null`; thread through to `tracker.listAlerts`. Default `null` keeps the global feed unchanged.
- Touches: `src/shared/api/ibkr.ts` — extend `tracker.listAlerts` typing to include the `symbol` filter.
- Touches: `src-tauri/src/ibkr/commands/tracker.rs` — add optional `symbol: Option<String>` to the `tracker_list_alerts` command; pass through to the underlying query (most likely a single `WHERE symbol = ?` branch in the SQL or repository call).
- Touches: `src-tauri/src/services/tracker_runner/` (or wherever the alerts read query lives) — add the optional symbol predicate; cargo test the new branch with a mocked `IbkrClientTrait`.
- Touches: `src/features/research/components/ResearchTab.tsx` — extract `NoteCard` into a shared file (e.g. `src/features/research/components/NoteCard.tsx`) if it isn't already, so `ResearchPanel` can render it without duplication.
- Touches: `src/features/workspace/components/WorkspaceTab.tsx` — replace the three placeholder panels with the real ones.

## Reuse (no new business logic this phase)

- `useResearchNotes({ symbol })` — drop-in.
- `useAlerts` — extended (symbol arg) but the alert list/event/enrich logic is unchanged.
- `useWatchlist`, `useTrackerEvents` — surface the row + live status changes.
- `AlertRow`, `TagEditor`, `SetupBadge`, `NoteCard` — render primitives.
- `AddToTrackerDialog` — opened via the existing app-level mount + `onAddClick`-style chain (or a small workspace-local store added here, decision below).

## Decisions to make in this phase

- **Alerts symbol filter site: backend vs. client.** Recommended **backend** (added to `tracker_list_alerts`). Pagination stays correct; older symbol-specific alerts remain reachable. Client filter sees only the current page, which is wrong UX for a workspace.
- **Inline "write a research note" action?** Recommended **no** for this phase. Notes are written by the LLM agent via MCP `write_research_note`; introducing a human-write surface adds an audit/review concern that's out of scope for the workspace arc. Defer.
- **Watchlist-meta editor scope.** Recommended **inline `TagEditor` + read-only setup badge + "edit in tracker" link**. Tag edits are the most common per-symbol need; deeper tracker actions (archive, change setup) stay on the Tracker page.
- **AddToTracker open mechanism from a panel.** Two options: (a) thread `onAddClick` through props, (b) introduce a small `useAddToTrackerOpen()` context. Recommended **(b)** — Phase 4 will need this from many sites; centralizing now avoids a refactor later. Implementation is ~10 LOC.
- **`NoteCard` shared placement.** Move out of `ResearchTab.tsx` into a sibling file. Both `ResearchTab` and `ResearchPanel` import the same component — single source of truth for note rendering.

## Exit criteria

- `pnpm typecheck && pnpm lint && pnpm test:run` green.
- `cargo test` passes including a new test for `tracker_list_alerts` with `symbol="X"` returning only X's alerts.
- Manual: tracked symbol → workspace → Research/Alerts/Watchlist-meta tabs each render the symbol's data; switching symbols re-fetches.
- Manual: untracked symbol → Research/Alerts show empty states; Watchlist-meta shows "Add to tracker" CTA that opens the prefilled `AddToTrackerDialog`.
- Manual: when a new alert fires for the active symbol (live tracker event), the Alerts panel patches without a manual refresh.
- Vitest per panel: scoped data renders; empty state renders; error state renders; panel reads symbol from `useWorkspace()` not from props (the panel signature accepts NO `symbol` prop — enforced by a vitest assertion).
- File-size: every new file ≤ 350 LOC; the workspace shell still ≤ 200.
- `EmptyState` component is the only empty-state surface used by the three panels (CI grep test optional but recommended).

## Gotchas

- **Alert enrichment listeners.** `useAlerts` listens for `alert-enriched` and `alert-dive-skipped` and patches by `alert_id`. The symbol-scoped variant must not break this (listeners still fire; matching alerts must already be in the panel's local state to be patched). Test with a fixture.
- **`onlyUnseen` + `filterKind`.** The existing `UseAlertsArgs` uses these for the global feed; the panel may want to default differently (e.g. show all kinds + all seen states for the active symbol, since it's already heavily filtered by symbol). Recommended: panel passes `filterKind=null, onlyUnseen=false`.
- **Backend filter SQL.** Verify the alerts query's existing index supports `(symbol, created_at)` lookups. If not, file a follow-up — performance is acceptable for v1 since alert volumes are small.
- **Watchlist refresh.** Adding/editing tags via `TagEditor` should propagate to other places that show tags (Tracker page) without a full reload. The existing `trackerVersion` bump in `App.tsx` is the existing pattern; reuse it.
- **`AddToTrackerDialog` prefill.** Pass `{ symbol, source: "workspace" }` so the audit trail distinguishes workspace adds from manual/scanner adds.
- **`NoteCard` migration.** Extracting it into a shared file is a pure move — preserve props, exports, and tests. Update `ResearchTab.tsx` to import from the new location.
- **Research note evidence refs.** Some refs link to alerts/news/setups by id. Phase 4 will turn these into navigators (alert id → workspace + Alerts tab); this phase keeps them as the existing label-only chips.
- **Empty-state CTA collision.** A panel's empty state may want a CTA ("Add to tracker", "Run scanner") — `EmptyState` should accept an optional `<button>` slot, not bake in CTA logic.
