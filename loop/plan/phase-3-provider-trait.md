# Phase 3 — Provider trait + AV adapter (pure refactor, no behavior change)

> Part of [Alpha Vantage strip-out: manual MCP fundamentals + IBKR news](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-02)

**Depends on:** 1 (so the AV path being abstracted is no longer self-poisoning)

**Goal:** Introduce a `FundamentalsProvider` trait abstracting the current `FinancialDataService::fetch_fundamental_data` call site. Wrap the existing AV implementation as `AlphaVantageFundamentalsProvider`. Route all three call sites (MCP tool, tracker, UI command) through `Arc<dyn FundamentalsProvider>`. End-state: behavior is byte-identical to today, but the system is one wiring change away from a different backend (which will be the composite provider in Phase 4).

This phase is intentionally a pure refactor. **No `fundamentals_source` settings flag** — the original plan had one to switch between AV and IBKR; with the IBKR arc abandoned, the runtime path is determined by the provider construction in `lib.rs`, not by config. Phase 4 wraps this AV provider in a `CompositeFundamentalsProvider`.

## Files

- New: `src-tauri/src/services/fundamentals_provider/mod.rs` — `FundamentalsProvider` trait + `FundamentalsError` enum.
- New: `src-tauri/src/services/fundamentals_provider/alpha_vantage.rs` — `AlphaVantageFundamentalsProvider` wrapping `FinancialDataService::fetch_fundamental_data`.
- New: `src-tauri/src/services/fundamentals_provider/test_support.rs` — `FakeFundamentalsProvider` for downstream tests.
- New: `src-tauri/src/services/fundamentals_provider/tests.rs` — trait shape tests, AV adapter round-trip via mock HTTP.
- Touches: `src-tauri/src/mcp/tools/fundamentals.rs` — take `Arc<dyn FundamentalsProvider>` (was `Arc<FinancialDataService>`).
- Touches: `src-tauri/src/mcp/handler.rs` — replace the `financial_service` field for fundamentals usage with the provider; keep `FinancialDataService` for news (and for the AV adapter's underlying impl).
- Touches: `src-tauri/src/ibkr/commands/analysis.rs` — use injected provider; **remove the silent mock-data fallback** (Hard Invariant #5).
- Touches: `src-tauri/src/lib.rs` — construct `AlphaVantageFundamentalsProvider`, `app.manage(Arc<dyn FundamentalsProvider>)`. The provider is the AV adapter directly in this phase; Phase 4 wraps it in a composite.

## Reuse

- Existing `IbkrClientTrait` pattern (`ibkr/mocks.rs`) — model the dyn-trait + mock structure on this.
- Existing `FundamentalData` shape — do NOT change it during this phase; it is the contract the trait preserves.
- Existing `FinancialDataService::fetch_fundamental_data` — wrap, don't rewrite. AV provider is a thin adapter.

## Decisions to make in this phase

- **Error variants.** `FundamentalsError` needs at minimum: `RateLimited { retry_after: Option<Duration> }`, `NotConnected`, `ParseError(String)`, `NotFound(String)`, `Other(String)`. Phase 4 will add `DailyBudgetExhausted` and `PerSymbolBudgetExhausted` for the composite provider's AV-side guards. Default: include those variants now (forward-compatible) but only `RateLimited`, `NotConnected`, `ParseError`, `NotFound`, `Other` are reachable from the AV adapter.
- **`FakeProvider` location.** `pub mod test_support` (mirroring `mcp/tools/test_support.rs`). Default: yes, so tests in other crates can use it.
- **Mock-fallback removal UX.** `analysis.rs` previously returned mock data when `ALPHA_VANTAGE_API_KEY` was unset. Removing it (per Invariant #5) means an unset key now surfaces as `FundamentalsError::Other("Alpha Vantage API key not configured")`. Decide whether to also update the UI in this phase or just ship the backend change with a clear error. Default: backend-only here; UI affordance lands in Phase 4 alongside the new "no fundamentals — paste some" empty state.
- **No tracker dependency.** Tracker does not currently call `fetch_fundamental_data` (verified 2026-05-02). The trait wiring touches `analysis.rs` and `mcp/tools/fundamentals.rs` only. **Do not** add tracker integration in this phase; Hard Invariant #6 forbids it.

## Exit criteria

- Every previously-passing cargo + vitest test still passes — this phase is a pure refactor.
- New test: MCP `get_fundamentals` invoked with a `FakeFundamentalsProvider` returning canned data round-trips through the tool and emerges identical at the JSON-RPC boundary.
- New test: `analysis.rs` UI command surfaces a typed error (mapped to a stable string) instead of returning mock data when AV fails. Error string is stable enough for the frontend to switch on.
- Grep on the fundamentals path (`mcp/tools/fundamentals.rs`, `ibkr/commands/analysis.rs`) shows zero direct `FinancialDataService::fetch_fundamental_data` references — only the AV adapter touches it. The news path still uses `FinancialDataService` directly.
- Grep on `tracker_runner/`, `strategies/`, `services/eod_scheduler/`, `services/intraday_scheduler/` returns zero references to `FundamentalsProvider` or `fetch_fundamental_data` (Hard Invariant #6 — tracker does not read fundamentals).
- Pre-commit clean.

## Gotchas

- **The `analysis.rs` mock-data fallback is intentional historic behavior** for "unset API key" UX. Removing it (per Invariant #5) means an unset `ALPHA_VANTAGE_API_KEY` now surfaces as an error in the UI instead of silently returning fake numbers. Document in the phase exit notes; ensure the error string is friendly enough for the UI.
- **`mcp/handler.rs` constructor.** Adding a new constructor parameter ripples into every test helper. Use a builder or default-ed field to keep the blast radius small.
- **`Arc<dyn Trait>` vs. generics.** Trait must be `Send + Sync + 'static` and dyn-compatible. The `async-trait` macro handles dyn-compat; verify with `cargo check`.
- **No `IbkrFundamentalsProvider` in this plan.** The original Phase 4 (IBKR provider impl) was abandoned along with Phase 2. The trait surface is designed for future flexibility, but the only impls planned are `AlphaVantageFundamentalsProvider` (this phase), `ManualFundamentalsProvider` + `CompositeFundamentalsProvider` (Phase 4), and `FakeFundamentalsProvider` (test support).
- **News path is NOT abstracted here.** AV news still flows through `FinancialDataService` directly. News abstraction lives in Phase 7.
