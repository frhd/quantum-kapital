# Questions / cross-phase issues — Ticker Intake Enrichment

Append-only log under `## Phase N (YYYY-MM-DD)` headings. Use for issues raised during execution that the phase intentionally did NOT fix: pre-existing flakes, scope-cut deferrals, decisions punted to a later phase.

Each entry should name the file/test/symbol so the next maintainer pass can find it.

---

<!-- entries land below as phases run -->

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
