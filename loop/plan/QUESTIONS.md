# Questions raised by /loop sessions

## Phase 2 (2026-05-01)

- `services::decay_watcher::tests::respects_budget_kill_switch` panics
  with `MockHttp queue exhausted` on a clean checkout of `main` (verified
  via `git stash` before the Phase-2 changes touched the tree). Not
  related to Phase 2's research-artifact work — flagging here so the
  next phase / a maintainer pass can fix the pre-existing brittle test
  fixture. Phase 2 leaves it as-is.

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
