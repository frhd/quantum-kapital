# Questions raised by /loop sessions

## Phase 2 (2026-05-01)

- `services::decay_watcher::tests::respects_budget_kill_switch` panics
  with `MockHttp queue exhausted` on a clean checkout of `main` (verified
  via `git stash` before the Phase-2 changes touched the tree). Not
  related to Phase 2's research-artifact work — flagging here so the
  next phase / a maintainer pass can fix the pre-existing brittle test
  fixture. Phase 2 leaves it as-is.
