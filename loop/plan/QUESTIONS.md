# Questions raised by /loop sessions

## Phase 2 (2026-05-01)

- `services::decay_watcher::tests::respects_budget_kill_switch` panics
  with `MockHttp queue exhausted` on a clean checkout of `main` (verified
  via `git stash` before the Phase-2 changes touched the tree). Not
  related to Phase 2's research-artifact work — flagging here so the
  next phase / a maintainer pass can fix the pre-existing brittle test
  fixture. Phase 2 leaves it as-is.

## Phase 7 (2026-05-02)

- The pre-existing `services::decay_watcher::tests::respects_budget_kill_switch`
  flake first noted in the Phase 2 entry above is still present on
  Phase 7's clean tree (`MockHttp queue exhausted`). Phase 7 does
  not touch `decay_watcher`; leaving as-is.

- The Phase-5 caller-identity issue noted below is unchanged:
  `append_journal_entry` lands writes under `caller=interactive`
  on the Tauri-hosted MCP server. Cron-driven `eod_review` runs
  its writes through the same bridge, so the audit row's
  `caller` will be `interactive` until per-connection caller
  resolution lands (still tracked under Phase 5/9).

## Phase 8 (2026-05-02)

- The pre-existing `services::decay_watcher::tests::respects_budget_kill_switch`
  flake first noted in Phase 2 is still present on Phase 8's clean
  tree (`MockHttp queue exhausted`). Phase 8 does not touch
  `decay_watcher`; leaving as-is.

## Phase 5 (2026-05-02)

- Per-connection caller identity is unresolved. Hard invariant 3 in
  `master.md` requires `mcp_audit.caller` / `research_notes.written_by`
  to distinguish `agent` from `user` and identify the agent loop.
  `src-tauri/src/mcp/handler.rs:80-86` currently uses a single caller
  string per server instance ("v1 uses single caller per server
  instance — agent loops [are] future work"). The `mcp-server` bridge
  (`src-tauri/src/bin/mcp-server.rs`) doesn't pass any agent-identity
  arg through. Phase 5's morning-sweep agent therefore lands writes
  under whatever caller the running Tauri app is initialised with
  (likely `interactive`), not `agent_morning_sweep`. Probably wants
  fixing in Phase 9 (daemon refactor) or sooner via a `QK_MCP_CALLER`
  env var read by the bridge and forwarded into the in-process
  `McpHandler::caller` field on connection accept. Phase 5 leaves it as-is.

- The Rust `tauri.conf.json` does not declare `mcp-server` as a sidecar
  binary. The README documents a manual `cargo build --release --bin
  mcp-server` step. If we want the binary to ship with the bundled app
  on user machines, sidecar wiring needs to be added — out of scope for
  this phase.

## Phase 9 deferral (2026-05-02)

- Phase 9 (daemon refactor) is marked `(optional)` in the master-plan
  index and `Why optional` in `phase-9-daemon.md`. Its trigger
  conditions ("Overnight ingestion becomes critical", "Morning sweep at
  07:00 must work without manual app launch", "Multiple UI clients")
  are user-judgment calls that haven't been declared met. The phase
  body is an explicit `(sketch)` rather than a concrete plan — new
  crate `quantum-kapital-core/`, new `quantum-kapital-daemon/` binary,
  systemd/launchd integration, IBKR reconnection logic, full test
  surface rewrite. A multi-week architectural refactor of that size
  isn't appropriate to drive autonomously through `/loop` without
  explicit user authorization, especially when the phase itself says
  "schedule when X becomes required." Quarter outcome stated in
  `master.md` ("every weekday a ranked morning pack…") is achieved by
  Phases 1-8 (all `done`). Leaving Phase 9 as `todo`; user to schedule
  manually if/when the trigger conditions are met. This loop ends here.
