# Cross-phase questions / open issues

Logged during phase execution per `loop/plan/master.md`. Each entry: who
found it, when, and the safer interpretation taken so the loop could
continue. Resolve and prune as phases progress.

---

## P1: `decay_watcher::tests::respects_budget_kill_switch` — pre-existing failure on `main`

- **Found:** Phase 1, 2026-05-02
- **Symptom:** `cargo test --lib services::decay_watcher::tests::respects_budget_kill_switch`
  panics with `MockHttp queue exhausted` at `src/services/decay_watcher/tests.rs:59`.
- **Reproducible on `main`** (verified with `git stash` before Phase 1
  changes). Not introduced by the AV burn-fix work; the failing test
  has nothing to do with the AV path.
- **Safer interpretation:** treat as an unrelated test-suite regression
  on `main`; do not block Phase 1 on it. The Phase 1 exit criteria say
  `cargo test` green; the underlying reason here is in the LLM decay
  watcher mock setup, which is orthogonal.
- **TODO:** investigate independently (likely a stale fixture / missing
  enqueue after a recent prompt change). Phase 1 commits do not touch
  `decay_watcher/`.
