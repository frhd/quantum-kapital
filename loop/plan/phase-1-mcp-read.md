# Phase 1 — MCP server (read-only tools)

> Part of [Quantum Kapital → Autonomous Researcher](master.md) quarter roadmap. See index for invariants, defaults, and cross-cutting references.

**Status:** done (commit 7992d46, 2026-05-01)

**Depends on:** none (foundation phase)

**Goal:** Stand up a stdio MCP server as a Tauri sidecar. Wrap existing read APIs as MCP tools. Connect Claude Code, prove an end-to-end interactive research session.

## Files

- New: `src-tauri/src/mcp/mod.rs`
- New: `src-tauri/src/mcp/server.rs` — stdio JSON-RPC 2.0 transport
- New: `src-tauri/src/mcp/tools/reads.rs`
- New binary: `src-tauri/src/bin/mcp-server.rs`
- Touches: `src-tauri/Cargo.toml` — add MCP SDK or build over JSON-RPC stdio
- Touches: `src-tauri/tauri.conf.json` — sidecar config to spawn MCP binary
- Touches: `src-tauri/src/lib.rs` — wire MCP server lifecycle into Tauri startup

## Tools exposed (read)

| Tool | Wraps |
|---|---|
| `get_positions` | `IbkrClientTrait::get_positions` |
| `get_account_summary` | `IbkrClientTrait::get_account_summary` |
| `get_watchlist` | `TrackerService::list` |
| `get_setups(filter)` | setups DAL with `status`, `since`, `symbol` filters |
| `get_alerts(filter, since?)` | alerts DAL |
| `get_bars(symbol, timeframe, lookback)` | `HistoricalDataService` (cache-first) |
| `get_quote(symbol)` | `QuoteService` (live IBKR snapshot) |
| `get_fundamentals(symbol)` | `FinancialDataService` (Alpha Vantage cache) |
| `get_news(symbol, since)` | `news_cache` reads with verdicts joined |
| `run_scanner(profile)` | `scan_one_shot` in `ibkr/client/streams.rs` |
| `get_llm_budget_status` | `llm_calls` ledger sum vs `daily_llm_budget_usd` |

## Reuse (no new business logic this phase)

All existing service traits. This phase is a pure transport/adapter layer over what's already in `services/`. No new SQL, no new external APIs.

## Per-tool checkpoint

Before moving on from any new tool, drive it once through `claude mcp` with a natural-language ask that exercises it. Schema and description quality only surface under a real LLM caller and are cheapest to fix at the moment of writing.

## Decisions to make in this phase

- **MCP SDK vs hand-rolled JSON-RPC.** Check for an `mcp` Rust crate; if rough, JSON-RPC 2.0 over stdio is ~200 lines. Decide in first day.
- **Tool error model.** Anthropic MCP convention: errors are `{isError: true, content: [...]}` rather than JSON-RPC errors. Be deliberate.
- **Schema docs.** Each tool needs a JSON schema for inputs + a description string Claude will read. Treat these like UX copy — they determine tool calling quality.

## Exit criteria

- `claude mcp` (or equivalent stdio config) connects to the sidecar at app launch.
- From a Claude Code session: "What setups are active and how do their fundamentals look?" → real answer with multiple visible tool calls and live data.
- Unit tests for each tool with a mocked service (use existing `MockIbkrClient` patterns in `src-tauri/src/ibkr/mocks.rs`).
- Integration test: spawn MCP binary, send JSON-RPC `tools/list` and one `tools/call`, assert structured response.

## Gotchas

- **Tauri sidecar lifecycle:** if the desktop app crashes the MCP server dies. Document this for Phase 5; v2 fix is Phase 9 daemon.
- **stdio buffering:** unbuffered stderr is critical for debugging; ensure `tracing` writes to stderr, JSON-RPC to stdout.
- **Tool surface drift:** every read added later (Phases 3+) extends `tools/reads.rs`. Keep tool registration declarative so it scales.
