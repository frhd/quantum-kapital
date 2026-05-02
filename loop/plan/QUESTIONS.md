# Questions / cross-phase issues — Unified Ticker Workspace

Append-only log under `## Phase N (YYYY-MM-DD)` headings. Use for issues raised during execution that the phase intentionally did NOT fix: pre-existing flakes, scope-cut deferrals, decisions punted to a later phase.

Each entry should name the file/test/symbol so the next maintainer pass can find it.

---

## Phase 4 (2026-05-03)

- **EvalTab has no per-symbol cells to wire** —
  `phase-4-universal-nav.md` calls for symbol cells in
  prediction/outcome rows in `src/features/eval/components/EvalTab.tsx`
  to navigate `(symbol, "history")`. The current EvalTab renders only
  conviction-bucket calibration tables and per-bucket cost breakdowns;
  there are no per-symbol prediction or outcome rows on screen. No
  change made — punt to a later eval-tab redesign that adds a
  per-prediction list. Existing surface is unaffected.


## Phase 2 (2026-05-02)

- **Pre-existing flake: `services::decay_watcher::tests::respects_budget_kill_switch`** —
  fails on a clean main tree (verified by stashing all phase-2 changes
  and re-running). Panics with `MockHttp queue exhausted` at
  `src-tauri/src/services/decay_watcher/tests.rs:59`. Not introduced by
  this phase; deferred to a separate fix that owns the decay_watcher
  area.

