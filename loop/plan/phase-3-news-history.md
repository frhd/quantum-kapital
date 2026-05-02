# Phase 3 — News + History tabs

> Part of [Unified Ticker Workspace](master.md). See index for invariants.

**Status:** todo

**Depends on:** 1

**Goal:** Add the **News** and **History** panels. News reads the existing `news_cache` SQLite table per symbol with whatever `news_interpreter` verdict is attached — producer-agnostic (robust to the AV→IBKR migration in `loop/plan/`). History reuses `useEvalDashboard` (or a per-symbol sibling) to surface predictions + outcomes scoped to the active symbol. After this phase, the workspace covers all read-only per-symbol slices the app already collects.

This phase can run in parallel with Phase 2 on a separate branch — they touch different panels and different backend commands.

## Files

- New: `src/features/workspace/components/panels/NewsPanel.tsx` — list of `NewsItem`s for the symbol with verdict chip + headline + source + timestamp.
- New: `src/features/workspace/components/panels/HistoryPanel.tsx` — predictions table + outcomes table (or a single combined timeline) for the symbol, with an accuracy headline.
- New: `src/features/workspace/hooks/useTickerNews.ts` — wraps the per-symbol news Tauri command; refresh listener if a `news-cached` event exists.
- New: `src/features/workspace/hooks/useTickerHistory.ts` — wraps `get_outcomes(symbol)` + `get_prediction_history(symbol)`; either reuses `useEvalDashboard` with a symbol filter or composes the two underlying API calls directly.
- New: tests for each panel + each hook.
- Touches: `src/shared/api/ibkr.ts` — wrappers for any new commands (`getTickerNews`, `getTickerOutcomes`, `getTickerPredictionHistory` if they aren't already wrapped).
- Touches (likely): `src-tauri/src/ibkr/commands/` — expose per-symbol read commands to the UI. The MCP server already has equivalent symbol-keyed read tools (`get_news`, `get_outcomes`, `get_prediction_history`); this phase mirrors those onto the Tauri command surface so the frontend stays inside `shared/api/*.ts`.
- Touches: `src/features/eval/hooks/useEvalDashboard.ts` — accept optional `symbol?: string | null` arg if reused (alternative: keep it global and write a new sibling hook). Decision below.
- Touches: `src/features/workspace/components/WorkspaceTab.tsx` — swap the News/History placeholders for real panels.

## Reuse (no new business logic this phase)

- `news_cache` SQLite table — schema unchanged.
- `news_interpreter` verdict (`news_verdict_json`) — display only; do not fire new LLM calls from the panel.
- `LlmService` — not invoked by this panel (panels are read-only).
- MCP read tool semantics (`get_news`, `get_outcomes`, `get_prediction_history`) — mirror to Tauri commands; same ordering, pagination, types.
- `useEvalDashboard` — composed by the History panel (with symbol filter) if extension is cheaper than a sibling hook.

## Decisions to make in this phase

- **Per-symbol news command shape.** Recommended: `get_ticker_news(symbol, limit)` Tauri command returning `Vec<NewsCacheRow>`. Cleaner than reusing the existing global `get_news` if it isn't already symbol-keyed at the command layer.
- **History granularity.** Recommended: headline accuracy + last N predictions (with realized outcome attached) + link "Open in Eval" for the full table. Avoids re-implementing the entire eval dashboard inside the panel.
- **Verdict display.** Recommended: render the structured verdict fields (sentiment, severity, key facts) inline if present; raw JSON otherwise. Keep it scannable.
- **`useEvalDashboard` extension vs. new hook.** Recommended: extend `useEvalDashboard` with `{ symbol?: string | null }` if the change is mechanical (one `WHERE` predicate); otherwise write a sibling `useTickerEvalSummary`. Decide after reading `useEvalDashboard.ts`.
- **News link-out vs. modal.** News rows link to original article URLs in a new tab (Tauri's `shell` plugin or `window.open`). No in-app modal; not worth the surface area.

## Exit criteria

- `pnpm typecheck && pnpm lint && pnpm test:run` green.
- `cargo test` covers the new per-symbol commands (if added): symbol filter returns only matching rows; empty result when no news/no predictions.
- Manual: ticker with cached news → News panel shows recent items with verdicts; ticker with no news → empty state; verdict-pending row shows "pending verdict" cleanly.
- Manual: ticker with predictions/outcomes → History panel shows accuracy headline + last N rows; ticker with no predictions → empty state.
- Manual: clicking a news item opens the source URL externally (no in-app navigation regression).
- Vitest: `NewsPanel` and `HistoryPanel` render fixture data; symbol-scoped; empty + error states; both consume `useWorkspace()` for the active symbol (no `symbol` prop).
- News panel produces no AV HTTP traffic in tests (the producer is the existing scheduler / future IBKR provider; the panel reads the cache).
- File-size: every new file ≤ 350 LOC.

## Gotchas

- **Cache freshness.** `news_cache` rows may be hours-to-days old depending on producer schedule. Surface the row's `cached_at` timestamp so the user can judge freshness; do not silently look stale.
- **Verdict absence.** `news_verdict_json` is null until the interpreter has run. Render a neutral chip ("pending") rather than blank space.
- **Producer migration boundary.** The AV→IBKR news provider work in `loop/plan/` changes who writes to `news_cache`, not the schema. The panel must NOT call any AV adapter directly — only the cache (via the new Tauri command). Add a vitest grep that asserts no AV SDK / AV URL string appears in the workspace.
- **Outcomes lag.** Predictions become outcomes only after their evaluation window. Headline accuracy must distinguish "still pending" vs. "resolved" rows so a fresh symbol with all-pending predictions doesn't look like 0% accuracy.
- **Eval dashboard refactor cost.** If extending `useEvalDashboard` cascades into many call sites (the global `EvalTab`), prefer a sibling per-symbol hook to keep the blast radius local.
- **Pagination.** News and history can be long-tailed for hot symbols. Cap at e.g. 50 rows in the panel + "Open in News/Eval" deep-link for the full feed (Phase 4 wires the deep-link).
- **Empty-state copy.** Distinguish "no news for this symbol" from "no news cached anywhere yet" — the latter signals a producer outage, not a quiet symbol.
