# Backend (src-tauri)

Rust backend for the Tauri 2 app. Cross-cutting rules in `../CLAUDE.md`.

## Common commands

All cargo commands need `--manifest-path src-tauri/Cargo.toml` from the repo root (no workspace at the root). For longer backend sessions, `cd src-tauri/` once and run the bare forms below.

```bash
cargo check
cargo test
cargo test ibkr::                       # by module path
cargo test -- test_specific_function    # single test
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

## Layering (`src/`)

```
config/        AppConfig + SettingsState (JSON persisted to OS app-data dir)
events/        EventEmitter → Tauri events; AppEvent enum is the contract with the UI
storage/       SQLite via rusqlite + r2d2 pool, refinery migrations under migrations/
ibkr/          IBKR adapter: client/ (TWS/Gateway — directory module split by domain:
                 mod.rs struct + connection lifecycle, market_data.rs, orders.rs,
                 historical.rs, streams.rs + StreamHandle),
               commands/ (Tauri handlers),
               types/ (domain types per concern), state.rs (IbkrState — the shared root),
               mocks.rs (MockIbkrClient — the IbkrClientTrait test seam)
strategies/    StrategyDetector trait + MarketContext + SetupCandidate + DetectorRegistry,
               one detector per subdir (breakout / episodic_pivot / parabolic_short)
services/      Business logic. Each service is constructed in lib.rs and managed via
               app.manage(...) so Tauri commands can fetch them via State<T>.
middleware/    Cross-cutting: RateLimiter, HistoricalRateLimiter, logging
mcp/           In-process MCP server (handler, server, transport, tools, ibkr_seam) —
               the read-only / ack-only API surface for external MCP clients
bin/           Standalone binaries (`mcp-server.rs` = stdio↔socket bridge)
utils/         Calendar (RTH/holidays), shared helpers
lib.rs         Tauri setup. Wires Db → IbkrState → services → schedulers and registers
               every #[tauri::command] handler.
```

`lib.rs::run` is the source of truth for service composition. Read it before adding a new service — most additions are: define the service in `services/`, construct it in `run()`, `app.manage(...)` it, then add a Tauri command in `ibkr/commands/` that pulls it via `State<Arc<MyService>>`.

## `IbkrState` and stream handles

`ibkr/state.rs` holds `Arc<IbkrClient>`, `Arc<EventEmitter>`, the SQLite `Arc<Db>`, the tracker services, and several stream handles. All long-running streams follow the same pattern: `*_handle: Arc<RwLock<Option<StreamHandle>>>`, with start methods that stop-then-replace. Mirror this pattern for any new stream.

## Tracker subsystem

Watchlist → detectors → LLM enrichment → alerts pipeline:

1. **Schedulers** (`services/eod_scheduler`, `services/intraday_scheduler`) tick on a calendar-aware schedule and call `TrackerRunner`.
2. **`TrackerRunner`** (`services/tracker_runner`) fetches bars (`HistoricalDataService`) and news (`FinancialDataService`), builds `MarketContext`, runs the `DetectorRegistry`, persists `SetupCandidate` rows, drives the state machine, and emits `SetupDetected`.
3. **LLM enrichment** (`services/thesis_generator`, `services/decay_watcher`, `services/news_interpreter`) calls `LlmService` (`services/llm_service`), which enforces a daily USD budget against the `llm_calls` ledger and re-emits enriched events. Two transport backends ship: `Anthropic` (default — POST api.anthropic.com) and `ClaudeCli` (spawns `claude -p` under the user's Claude Code subscription). Selected via `QK_LLM_BACKEND=anthropic|claude_cli`. Under `claude_cli` the ledger's `cost_usd` is best-effort (parsed from `total_cost_usd` in the CLI envelope when present, otherwise computed from token counts via `prices::cost_usd`); token counts stay accurate, so the kill-switch still trips deterministically. Cache breakpoints (`SystemBlock.cache=true`) are no-ops in CLI mode — the CLI doesn't expose ephemeral cache control.
4. **State machine** (`services/tracker_state_machine`) owns `watching → in_play → cool_down` transitions per ticker.

SQLite tables (see `src/storage/schema.sql`): `tracked_tickers`, `setups`, `alerts`, `bars_cache`, `news_cache`, `llm_calls`. The pre-existing file-based `cache_service.rs` (JSON, 7-day TTL for fundamentals/projections) is intentionally **not** migrated to SQLite.

## MCP server

External MCP clients (Claude Code, etc.) talk to the running app via two pieces:

- **`bin/mcp-server.rs`** — a standalone stdio↔unix-socket bridge. The MCP client spawns this binary; it just shovels JSON-RPC bytes between its stdin/stdout and the local socket bound by the running Tauri app. Stdout is reserved for the protocol stream — diagnostics go to stderr only. Default socket path is derived from the Tauri identifier (`com.quantyc.qqk` → `…/mcp.sock`); override with `QK_MCP_SOCKET`.
- **`mcp/`** — the in-process server hosted inside Tauri. Holds the protocol handler, transport, the `ibkr_seam` (read-only adapter over `IbkrClientTrait`), and the tool registry under `mcp/tools/`. New MCP tools are added here, not in `ibkr/commands/`.

The MCP surface is **read-only plus an `ack_alert` rail** — no order tools, ever. Acks are audited through `services/mcp_audit/`. Surveillance-only invariant from the root CLAUDE.md applies here too: a tool that places an order would violate the project contract.

Integration tests live at `tests/mcp_tool_call.rs` and `tests/mcp_surveillance_audit.rs`; run them like any other cargo test.
