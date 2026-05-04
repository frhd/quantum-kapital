# Questions / cross-phase issues — Trade history visibility

Append-only log under `## Phase N (YYYY-MM-DD)` headings. Use for issues raised during execution that the phase intentionally did NOT fix: pre-existing flakes, scope-cut deferrals, decisions punted to a later phase.

Each entry should name the file/test/symbol so the next maintainer pass can find it.

---

<!-- entries land below as phases run -->

## Phase 1 (2026-05-04)

- `services::decay_watcher::tests::respects_budget_kill_switch` panics with
  "MockHttp queue exhausted" at `src/services/decay_watcher/tests.rs:59`.
  Reproduces on `main` *without* any Phase 1 changes (verified via
  `git stash` + re-run), so it's a pre-existing flake unrelated to the
  executions work. Did not touch `decay_watcher` here. Leaving as-is.
- `cargo fmt` flagged a pre-existing drift in
  `src-tauri/src/ibkr/client/market_data.rs` (two block formatting tweaks
  inside `snapshot_blocking` and `streaming_drain_blocking`). Picked up
  in a sibling chore commit so the Phase 1 diff stays focused on the
  executions adapter; no logic change.
