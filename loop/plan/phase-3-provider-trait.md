# Phase 3 — Provider trait + AV adapter (pure refactor, no behavior change)

> Part of [Alpha Vantage → IBKR Reuters](master.md). See index for invariants.

**Status:** todo

**Depends on:** 1 (so the AV path being abstracted is no longer self-poisoning), 2 (so the trait shape accounts for IBKR-specific error variants discovered in the spike)

**Goal:** Introduce a `FundamentalsProvider` trait abstracting the current `FinancialDataService::fetch_fundamental_data` call site. Wrap the existing AV implementation as `AlphaVantageFundamentalsProvider`. Route all three call sites (MCP tool, tracker, UI command) through `Arc<dyn FundamentalsProvider>`. Add a `fundamentals_source` settings flag, defaulted to `"alpha_vantage"`. End-state: behavior is byte-identical to today, but the system is one wiring change away from a different backend.

## Files

- New: `src-tauri/src/services/fundamentals_provider/mod.rs` — `FundamentalsProvider` trait + `FundamentalsError` enum.
- New: `src-tauri/src/services/fundamentals_provider/alpha_vantage.rs` — `AlphaVantageFundamentalsProvider` wrapping `FinancialDataService::fetch_fundamental_data`.
- New: `src-tauri/src/services/fundamentals_provider/test_support.rs` — `FakeFundamentalsProvider` for downstream tests.
- New: `src-tauri/src/services/fundamentals_provider/tests.rs` — trait shape tests, AV adapter round-trip via mock HTTP.
- Touches: `src-tauri/src/mcp/tools/fundamentals.rs` — take `Arc<dyn FundamentalsProvider>` (was `Arc<FinancialDataService>`).
- Touches: `src-tauri/src/mcp/handler.rs` — replace the `financial_service` field for fundamentals usage with the provider; keep `FinancialDataService` for news.
- Touches: `src-tauri/src/ibkr/commands/tracker.rs:42` — use injected provider.
- Touches: `src-tauri/src/ibkr/commands/analysis.rs:32-44` — use injected provider; **remove the silent mock-data fallback** (Hard Invariant #5).
- Touches: `src-tauri/src/lib.rs` — construct `AlphaVantageFundamentalsProvider` and `app.manage(Arc<dyn FundamentalsProvider>)`. Read `fundamentals_source` from settings; for now the only valid value is `"alpha_vantage"`.
- Touches: `src-tauri/src/config/settings.rs` — add `fundamentals_source: String` field, default `"alpha_vantage"`. Validate at load time (unknown value → warn + fall back to default).
- Touches: `src-tauri/src/mcp/tools/test_support.rs` — swap test-helper construction to use `FakeFundamentalsProvider`.

## Reuse

- Existing `IbkrClientTrait` pattern (`ibkr/mocks.rs`) — model the dyn-trait + mock structure on this.
- Existing `FundamentalData` shape — do NOT change it during this phase; it is the contract the trait preserves.
- Existing `FinancialDataService::fetch_fundamental_data` — wrap, don't rewrite. AV provider is a thin adapter.

## Decisions to make in this phase

- **Error variants.** `FundamentalsError` needs at minimum: `RateLimited { retry_after: Option<Duration> }`, `NoSubscription`, `NotConnected`, `ParseError(String)`, `NotFound(String)`, `Other(String)`. Decide if `RateLimited` always carries a retry hint. Default: yes, so the agent loop can backoff intelligently.
- **`FakeProvider` location.** `#[cfg(test)] mod` vs. `pub mod test_support` (mirroring `mcp/tools/test_support.rs`). Default: `pub mod test_support` so integration tests in other crates can use it.
- **Settings validation.** What happens if `fundamentals_source` is an unknown string? Default: log a warn and fall back to `"alpha_vantage"`.
- **Mock-fallback removal UX.** `analysis.rs` previously returned mock data when `ALPHA_VANTAGE_API_KEY` was unset. Surfacing this as `FundamentalsError::Other("Alpha Vantage API key not configured")` may need a frontend toast or banner. Decide whether to also update the UI in this phase or just ship the backend change with a clear error.

## Exit criteria

- Every previously-passing cargo + vitest test still passes — this phase is a pure refactor.
- New test: `MCP get_fundamentals` invoked with a `FakeFundamentalsProvider` returning canned data round-trips through the tool and emerges identical at the JSON-RPC boundary.
- New test: `analysis.rs` UI command surfaces a typed error (mapped to a stable string) instead of returning mock data when AV fails. Error string is stable enough for the frontend to switch on.
- Grep on the fundamentals path (`mcp/tools/fundamentals.rs`, `ibkr/commands/{analysis,tracker}.rs`) shows zero direct `FinancialDataService::fetch_fundamental_data` references — only the AV adapter touches it. The news path still uses `FinancialDataService` directly.
- Settings file generated from a fresh launch contains `"fundamentals_source": "alpha_vantage"`.
- Pre-commit clean.

## Gotchas

- **The `analysis.rs` mock-data fallback is intentional historic behavior** for "unset API key" UX. Removing it (per Invariant #5) means an unset `ALPHA_VANTAGE_API_KEY` now surfaces as an error in the UI instead of silently returning fake numbers. Document in the phase exit notes; ensure the error string is friendly enough for the UI.
- **`mcp/handler.rs` constructor.** Adding a new constructor parameter ripples into every test helper. Use a builder or default-ed field to keep the blast radius small.
- **`Arc<dyn Trait>` vs. generics.** Trait must be `Send + Sync + 'static` and dyn-compatible. The `async-trait` macro handles dyn-compat; verify with `cargo check`.
- **Don't preemptively introduce `IbkrFundamentalsProvider` here.** Phase 4 owns that. Phase 3 ships even if Phase 4 hasn't started — that's the value of clean phasing.
- **News path is NOT abstracted.** AV news still flows through `FinancialDataService` directly. Keep it that way; news migration is out of scope.
