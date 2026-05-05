# Open questions / cross-phase issues

## Pre-existing test failures (not introduced by this plan)

- **`services::decay_watcher::tests::respects_budget_kill_switch` panics with
  "MockHttp queue exhausted"** — fails deterministically on `4b14527` (parent
  of the Phase 3 work) and on `3cb7198`. Not introduced by Phases 1/2/3.
  Owner: out-of-scope follow-up; track separately. The rest of the lib test
  suite is green.
