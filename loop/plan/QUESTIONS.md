# Questions / cross-phase issues — Ticker Intake Enrichment

Append-only log under `## Phase N (YYYY-MM-DD)` headings. Use for issues raised during execution that the phase intentionally did NOT fix: pre-existing flakes, scope-cut deferrals, decisions punted to a later phase.

Each entry should name the file/test/symbol so the next maintainer pass can find it.

---

<!-- entries land below as phases run -->

## Phase 2 (2026-05-03)

- **`written_by` discipline is client-side only.** The plan calls for
  baseline notes to land in `research_notes` with
  `written_by = "agent.ticker_intake"`. In practice, the
  `write_research_note` MCP tool overwrites the value with
  `self.caller`, which the in-process MCP server hardcodes to
  `"interactive"` (see `mcp/handler.rs:108` and the comment "Per-
  connection caller resolution is a future enhancement once the agent
  loops land in Phase 5/6"). All Phase 5/6 writers
  (`alert_dive`, `morning_sweep`) have the same gap. Phase 2 keeps
  `WRITER = "agent.ticker_intake"` as a Python constant for grep
  discipline + future migration, but does NOT add a `written_by`
  parameter to the MCP schema (out-of-scope; "no Rust changes this
  phase"). Cross-phase tracer #2 therefore verifies the call shape
  via the test fake, not against a live DB row. Filed as the next
  Rust follow-up: surface caller identity per MCP connection so the
  three agent loops can each stamp their own writer string.

- **Missing MCP read for "research notes per symbol in last N days".** The
  agent's reuse-note window is 7 days across all writers
  (`agent.ticker_intake`, `alert_dive`, `morning_sweep`), but the MCP
  surface exposes neither `list_research_notes` nor a per-symbol filter
  on `get_morning_pack` / `get_outcomes` that would surface baseline
  notes. The Rust service-layer has
  `services::research_notes::list_notes(ListNotesQuery {symbol, ...})`
  but no `#[tool]` wrapper. Phase 2 ships with two fallbacks per the
  master-plan instruction "use whatever the closest existing tool is —
  do not introduce a new MCP tool for this phase":
  1. An optional `list_research_notes(symbol, since)` hook on the MCP
     adapter — production currently no-ops it (returns `None`), tests
     provide a fake implementation. Mirrors the `alert_dive.py` shape.
  2. A daemon-lifetime in-memory `seen_at` cache that prevents the
     loop from re-writing a baseline note for the same symbol within
     `reuse_note_window_days`. Survives ticks; reset on restart.
  Trade-off: a daemon restart inside the 7-day window may rewrite one
  note per primed symbol on first tick. Acceptable for Phase 2 because
  (a) `last_primed_at` gates eligibility, and (b) per-loop budget caps
  the spend. If the rewrite becomes a real cost or eval pollutes the
  data set, lift `list_research_notes` into a real `#[tool]` in a
  follow-up phase.

## Phase 1 (2026-05-03)

- **Pre-existing flake:** `services::decay_watcher::tests::respects_budget_kill_switch`
  panics with "MockHttp queue exhausted" on `main` independent of this
  phase's changes (verified via `git stash` + `cargo test
  services::decay_watcher::tests::respects_budget_kill_switch`).
  The phase did not investigate or fix; the kill-switch path itself is
  untouched. File a follow-up against `decay_watcher/tests.rs:59` if the
  flake bites the next person; test name suggests the mock HTTP queue
  needs an extra response queued for the new ordering.
- **Cache-dir cleanup in `set_fundamentals` test helper:** the inline
  cache-dir fixture in `mcp/tools/set_fundamentals.rs::handler_with_composite`
  uses a wall-clock-nanosecond suffix and is never reclaimed during the
  test run (TempDir would race the spawned primer task that the
  set_fundamentals path itself never invokes). Acceptable because the
  primer is wired but never called in this test. If a future test does
  drive prime through this builder, lift the per-test cache to a
  TempDir held alive for the test scope.
