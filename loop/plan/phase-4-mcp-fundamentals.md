# Phase 4 — MCP `set_fundamentals` write tool + manual store + composite provider

> Part of [Alpha Vantage strip-out: manual MCP fundamentals + IBKR news](master.md). See index for invariants.

**Status:** todo

**Depends on:** 3 (need `FundamentalsProvider` trait + `AlphaVantageFundamentalsProvider`)

**Goal:** Build the LLM-mediated manual fundamentals path. New SQLite-backed `ManualFundamentalsStore`. New MCP write tool `set_fundamentals` (audited via `services/mcp_audit/`, same rail as `ack_alert`). New `CompositeFundamentalsProvider` that reads in order: (1) manual store, (2) AV cache (fresh, then stale on rate-limit), (3) AV API (only if budget allows). Manual writes invalidate the AV cache row for that symbol so AV is never re-consulted for it. End-state: a user can paste fundamentals into a Claude Code session, the LLM extracts structured fields, the MCP tool persists them, and the analysis UI / `get_fundamentals` tool read from the manual store transparently.

This phase introduces a new write tool to the MCP surface. `services/mcp_audit/` records every call (caller, timestamp, symbol, source, diff vs. prior) so provenance is traceable.

## Files

- New: `src-tauri/migrations/V<next>__manual_fundamentals.sql` — refinery migration creating:
  ```
  CREATE TABLE manual_fundamentals (
    symbol TEXT PRIMARY KEY,
    as_of_date TEXT NOT NULL,        -- ISO 8601 date
    source TEXT NOT NULL,            -- free-form provenance string
    payload_json TEXT NOT NULL,      -- serialized FundamentalData
    written_at TEXT NOT NULL,        -- ISO 8601 timestamp
    written_by TEXT NOT NULL          -- MCP caller identifier (audit cross-ref)
  );
  CREATE INDEX idx_manual_fundamentals_written_at ON manual_fundamentals(written_at);
  ```
- New: `src-tauri/src/services/manual_fundamentals_store/mod.rs` — `ManualFundamentalsStore` struct with `get(symbol)`, `upsert(symbol, FundamentalData, as_of_date, source, written_by)`, `clear(symbol)`, `list_with_freshness()`.
- New: `src-tauri/src/services/manual_fundamentals_store/tests.rs` — round-trip tests + concurrent-write tests (SQLite handles via r2d2 pool).
- New: `src-tauri/src/services/fundamentals_provider/manual.rs` — `ManualFundamentalsProvider` thin adapter over `ManualFundamentalsStore` implementing `FundamentalsProvider`. Returns `FundamentalsError::NotFound(symbol)` when the store is empty for that symbol so the composite can fall through to AV.
- New: `src-tauri/src/services/fundamentals_provider/composite.rs` — `CompositeFundamentalsProvider` that holds `Arc<ManualFundamentalsProvider>` + `Arc<AlphaVantageFundamentalsProvider>` + `Arc<CacheService>`. `fetch(symbol)` tries manual → AV cache (with `read_ignoring_ttl` stale-fallback path from Phase 1) → AV API. Logs the source decision at info level for every call.
- New: `src-tauri/src/services/fundamentals_provider/composite_tests.rs` — fall-through ordering tests, manual-wins-over-cache test, AV-cache-stale-on-rate-limit test, manual-write-invalidates-AV-cache test.
- New: `src-tauri/src/mcp/tools/set_fundamentals.rs` — MCP write tool. Input schema generated from `FundamentalData` plus envelope (`symbol`, `as_of_date`, `source`, optional `notes`). Validation: symbol non-empty + matches `^[A-Z][A-Z0-9.\-]{0,9}$`, as_of_date parseable ISO 8601, source non-empty, payload validates against `FundamentalData` JSON Schema. On success: writes to store, invalidates AV cache row for symbol via `CacheService::clear_key`, audits via `services/mcp_audit/`, returns the diff vs. prior store entry (or "new entry") in the tool result.
- New: `src-tauri/src/mcp/tools/set_fundamentals_tests.rs` — schema validation tests, audit-row-written test, AV-cache-purged-on-write test, diff-in-response test.
- Touches: `src-tauri/src/services/cache_service.rs` — add `clear_key(key: &str) -> Result<(), Error>` if not already present (Phase 1's `read_ignoring_ttl` is the closest neighbor; see line ~96-101). Used by composite to purge AV cache on manual write.
- Touches: `src-tauri/src/services/mcp_audit/` — extend audit schema/handler to cover `set_fundamentals`. Audit row carries `tool="set_fundamentals", symbol, as_of_date, source, payload_hash, prior_payload_hash` so provenance is auditable end-to-end.
- Touches: `src-tauri/src/mcp/server.rs` (or wherever the tool registry lives) — register `set_fundamentals` as a write tool. Surface it in `tools/list` JSON-RPC response.
- Touches: `src-tauri/src/mcp/handler.rs` — provide `Arc<ManualFundamentalsStore>` + `Arc<MCPAuditService>` to the new tool.
- Touches: `src-tauri/src/lib.rs` — construct `ManualFundamentalsStore` (passes the `Db` pool), `ManualFundamentalsProvider`, `CompositeFundamentalsProvider`. **Replace** the bare `AlphaVantageFundamentalsProvider` from Phase 3 with the composite as the trait-managed instance. The AV provider is now an internal field of the composite, not the top-level managed instance.
- Touches: `src-tauri/src/storage/schema.sql` — add `manual_fundamentals` table to the dev-mode bootstrap schema if the project uses it (parallel to `news_cache` etc.).

## Reuse

- `services/cache_service.rs::CacheService` — both the existing `read` path (TTL-respecting) and the Phase 1 `read_ignoring_ttl` (stale-fallback). Add `clear_key` if missing.
- `services/mcp_audit/` — same audit rail as `ack_alert`. Extend schema if needed; do not introduce a parallel audit system.
- `storage/` SQLite + r2d2 pool — `ManualFundamentalsStore` borrows the existing `Arc<Db>` from `IbkrState` / `lib.rs` composition.
- `FundamentalData` shape — unchanged.
- `services/fundamentals_provider/test_support.rs::FakeFundamentalsProvider` from Phase 3 for any downstream tests that don't care which provider supplies the data.
- Phase 1 `AlphaVantageRateLimiter` — already wraps the AV client; the composite doesn't re-implement rate limiting.

## Decisions to make in this phase

- **Tool name.** `set_fundamentals` vs. `store_fundamentals` vs. `submit_fundamentals`. Default: `set_fundamentals` — symmetric with the existing `get_fundamentals` read tool.
- **Upsert semantics.** Replace entirely vs. merge field-level. Default: **replace** at the `FundamentalData` level. The LLM is the merger — if the user pastes a partial update, it's the LLM's job to fetch the existing payload via `get_fundamentals` first, merge, and submit the full new payload. Keeps the tool semantics simple and the audit diff meaningful.
- **JSON Schema strictness.** Reject extra fields vs. silently drop them. Default: **reject extra fields** so LLM hallucinations (extra keys, typo'd fields) surface as validation errors instead of silently being dropped.
- **Diff format in tool response.** Full structured diff vs. summary string. Default: structured diff (`{added: {...}, changed: {field: {from, to}}, removed: {...}}`) so the LLM and the user both see exactly what changed; surprising changes (5x movements, sign flips) get a `warning` field in the response.
- **Range/sanity checks.** What thresholds? Default: `pe_ratio >= 0`, `shares_outstanding > 0`, every numeric field finite (no NaN/Inf), warn (don't reject) if any numeric changed by more than 5x vs. prior.
- **Concurrency.** Two MCP clients writing the same symbol simultaneously. Default: SQLite handles it (last-write-wins under a single transaction); the diff in the second response will show the first write as the prior state. Acceptable.
- **MCP audit schema migration.** If `services/mcp_audit/` schema can't accommodate `set_fundamentals` cleanly (e.g., it assumes alert-shaped rows), add a refinery migration here. Default: read the existing schema first; extend it if a new column makes more sense than a parallel table.

## Exit criteria

- New SQLite migration applies cleanly on `cargo test` and on a fresh app launch.
- `ManualFundamentalsStore` round-trip test passes: `upsert(symbol, fd, ...)` then `get(symbol)` returns equivalent `FundamentalData` (modulo serde round-trip).
- `CompositeFundamentalsProvider` ordering test: with empty store + AV-mocked-to-return-X, `fetch` returns X. With store-has-Y + AV-would-return-X, `fetch` returns Y. With store-empty + AV-rate-limited + cache-has-stale-Z, `fetch` returns Z and logs the stale-source decision.
- MCP `set_fundamentals` validation test: payload missing required field → JSON-RPC error with field path; extra field → JSON-RPC error; well-formed payload → success + diff in result.
- MCP `set_fundamentals` cache-invalidation test: pre-populate AV cache with X for AAPL; call `set_fundamentals(symbol="AAPL", ...)` with payload Y; assert AV cache row for AAPL is gone; assert subsequent `get_fundamentals` returns Y.
- MCP audit row written for every successful `set_fundamentals` call. Audit row contains `tool, symbol, source, payload_hash, prior_payload_hash, written_by, written_at`.
- Tracer-bullet test from `tests/mcp_tool_call.rs`-style harness: `set_fundamentals(symbol="TEST", as_of_date="2026-05-02", source="manual test", current_metrics={...})` then `get_fundamentals(symbol="TEST")` returns the submitted data; AV transport mock asserts zero requests fired.
- Pre-commit clean.

## Gotchas

- **MCP write surface expansion.** `set_fundamentals` is the first non-`ack_alert` write tool added under this plan. Hard Invariant #3 (surveillance-only) is preserved because this writes operator-curated reference data, not market actions. Document in the tool description that it's user-mediated and audited; future write-tool additions need explicit invariant review.
- **JSON Schema generation from `FundamentalData`.** The `FundamentalData` Rust type uses `#[serde(rename_all = "camelCase")]` and several `#[serde(skip_serializing_if = "Option::is_none")]` attributes. The MCP schema must reflect these accurately. Use `schemars` crate or hand-roll the schema; verify with a known-good payload round-trip.
- **`schemars` may not be in `Cargo.toml`.** If hand-rolling the schema feels brittle, add `schemars` as a dependency and derive `JsonSchema` on `FundamentalData` and friends. Phase ships either way; document the choice in the phase commit body.
- **AV cache key shape.** `services/financial_data_service/mod.rs::fetch_av_function` constructs cache keys per (symbol, endpoint). `clear_key` for AV invalidation must clear all 3 endpoint keys for the symbol (`<SYMBOL>_overview`, `<SYMBOL>_income`, `<SYMBOL>_earnings`), not just one. Inspect the keying scheme before implementing `composite.rs`.
- **Audit schema impedance.** `services/mcp_audit/` was built around alert acknowledgment; its row shape may not naturally fit fundamentals writes. Read the existing schema; either extend the columns (preferred — single audit log) or add a sibling table (acceptable — more isolation but two audit surfaces). Don't introduce a third pattern.
- **Manual store overwrites are silent at the storage layer.** Make sure the audit captures `prior_payload_hash` so an accidental overwrite (e.g., LLM mis-extracts and overwrites a good payload) is recoverable from the audit row. Future "undo last write" tool is out of scope here but the audit data must support it.
- **Settings-flag temptation.** Resist adding a `manual_fundamentals_enabled: bool` flag. The composite provider IS the only fundamentals path after this phase; it doesn't toggle. If the user wants to disable manual writes, they don't call `set_fundamentals` — there's no global kill switch needed.
- **`StrategyContext.fundamentals` field.** The aspirational `Option<&FundamentalData>` field on `StrategyContext` (`src-tauri/src/strategies/context.rs:12`) is unused by every strategy and remains so under this plan. Hard Invariant #6 forbids the tracker from fetching fundamentals; do not wire it up here. Consider deleting the field in a separate cleanup PR if the tracker really won't ever consume fundamentals — but that's a separate decision, not in this phase's scope.
