# Phase 9 — Daemon refactor (optional)

> Part of [Quantum Kapital → Autonomous Researcher](master.md). See index for invariants.

**Status:** todo

**Depends on:** independent — schedule when overnight ingestion / app-closed sweeps become required.

**Goal:** Extract MCP server + schedulers + ingesters to a standalone Rust daemon. Tauri app becomes a thin UI client. Eliminates "desktop app must be open" constraint for cron jobs and continuous ingestion.

## Why optional

Big refactor. v1 cron-launches the Tauri app; for many users that's sufficient. Worth doing when:
- Overnight ingestion becomes critical (international markets, after-hours news).
- Morning sweep at 07:00 must work without manual app launch.
- Multiple UI clients (mobile companion app, browser dashboard) want to share one backend.

## Files (sketch)

- New crate: `quantum-kapital-core/` containing today's `services/`, `storage/`, `mcp/`, schedulers
- New binary: `quantum-kapital-daemon/` (systemd service on Linux, launchd plist on macOS)
- Tauri app refactored to connect to local daemon via Unix socket or local HTTP
- Schedulers move out of Tauri-managed tasks into daemon main loop
- MCP server runs as part of the daemon process (no longer Tauri sidecar)
- Tauri app becomes mostly read-only on shared state; writes go through daemon API

## Migration considerations

- **SQLite single-writer.** Daemon owns writes; UI is read-only or proxies through daemon. Use WAL mode + read connections from UI.
- **Connection pooling.** Daemon needs a long-lived IBKR connection that survives UI restarts.
- **State leak.** Today some state is in-memory in Tauri command handlers — audit and move to daemon.
- **Rolling deploy story.** Daemon update without losing IBKR connection? Probably acceptable to drop and reconnect; document the behavior.

## Exit criteria

- Daemon runs as a system service, restarts on crash.
- Tauri app can be closed and reopened without affecting schedulers, ingesters, or agent loops.
- Morning sweep at 07:00 fires regardless of UI state.
- Single config file for both daemon and UI; no duplicated state.

## Gotchas

- **macOS launchd quirks.** GUI vs system context matters for keychain access (relevant if you store any secrets there). Test on target deployment.
- **IBKR client behavior on long-running daemon.** TWS/Gateway can drop connections nightly during reset; daemon needs reconnection logic + back-pressure on dependent services.
- **Existing tests.** Most tests currently use Tauri-managed services; refactor will require test surface changes. Plan for a test sweep as part of the migration.
