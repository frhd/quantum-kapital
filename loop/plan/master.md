# Alpha Vantage Fundamentals → IBKR Reuters: Vendor Migration

## Context

Alpha Vantage's free tier (25 calls/day, 1 req/sec) is the current source for company fundamentals (OVERVIEW, INCOME_STATEMENT, EARNINGS) and ticker-tagged news sentiment. Logs from 2026-05-02 show the daily AV quota burning twice as fast as it should: the frontend `useProjections` hook calls `getFundamentalData` and `generateProjectionResults` in parallel, and the latter command internally re-fetches the same fundamentals — every UI projection lookup costs 6 AV requests instead of 3. There is also no in-flight coalescing across concurrent agent-loop callers, no per-vendor rate limiter, and no stale-cache fallback when AV returns its rate-limit `Information` payload (unlike news, which gracefully serves stale at `services/financial_data_service/news.rs:280-286`).

Even with all four bugs fixed, the free 25/day cap cannot carry the planned **100+ ticker** morning sweep. AV is on the agent-loop critical path: MCP `get_fundamentals` (`src-tauri/src/mcp/tools/fundamentals.rs:42`) and the tracker pipeline (`src-tauri/src/ibkr/commands/tracker.rs:42`) both reach it. Premium AV is $50/mo recurring; the user's IBKR account can carry Reuters Worldwide Fundamentals (~$5–15/mo) and already supplies market data + news.

The architectural change: replace the AV-coupled `FinancialDataService::fetch_fundamental_data` call site with a `FundamentalsProvider` trait. IBKR Reuters becomes the default implementation; AV stays as a fallback for one release cycle then is deleted. News stays on AV — separate decision, separate plan.

## End-state architecture

Two-implementation provider trait, IBKR by default. Same `FundamentalData` shape downstream so MCP tool, tracker, and UI command don't notice the swap.

| Subsystem | Responsibility |
|---|---|
| **`FundamentalsProvider` trait** | Single async method `fetch(symbol) -> Result<FundamentalData, FundamentalsError>`. Mock-friendly. |
| **`IbkrFundamentalsProvider`** (default) | Calls `req_fundamental_data` for ReportSnapshot + ReportsFinSummary + ReportsFinStatements + RESC, parses Reuters XML into `FundamentalData`. |
| **`AlphaVantageFundamentalsProvider`** (fallback) | Wraps the existing AV path. Kept one release cycle behind a settings flag, then deleted. |
| **`fundamentals_source` setting** | `"ibkr" \| "alpha_vantage"`. Default `"alpha_vantage"` until Phase 5 cutover. |
| **Caching** | Per-provider on-disk JSON cache (existing `CacheService`), 7-day TTL. Unchanged. |

## Hard invariants

1. **`FundamentalData` shape is the contract.** All providers return the same struct; no caller code (MCP tool, tracker, UI) cares about the source. Adding a field requires updating both providers in lockstep.
2. **Mock-friendly trait seam.** `FundamentalsProvider` is `Send + Sync + 'static` and dyn-compatible; tests use `FakeFundamentalsProvider`. The IBKR live impl is reached only via the existing `IbkrClientTrait` seam (`ibkr/mocks.rs`).
3. **Surveillance-only stays.** No fundamentals work touches order placement. The MCP tool surface is unchanged — `get_fundamentals` is the same tool with a different backend.
4. **Pre-commit sacred** — `cargo fmt --check`, `cargo clippy -D warnings`, `prettier`, `eslint`. Never `--no-verify`.
5. **No silent fallback to mock data on the migration path.** The existing `analysis.rs` "fall back to mock if AV fails" behavior is removed in Phase 3 — provider errors propagate as typed errors. Mock data exists for tests only.

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **IBKR fundamentals subscription:** Reuters Worldwide Fundamentals via TWS Account → Market Data Subscriptions. Required from Phase 2 onward.
- **IBKR API crate:** existing `ibapi = "2"` (sync feature). Phase 2 confirms `req_fundamental_data` is exposed; if not, Phase 2 picks an alternative.
- **Cache TTL:** 7 days per provider (unchanged).
- **Settings flag:** `fundamentals_source: "ibkr" | "alpha_vantage"`. Default `"alpha_vantage"` until Phase 5.
- **News stays on AV.** Out of scope. Revisit when AV news quota becomes painful.

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. AV burn fixes (stop the bleed) | [phase-1-av-burn-fixes.md](phase-1-av-burn-fixes.md) | — | in-progress (started 2026-05-02) |
| 2. IBKR Reuters spike (de-risk) | [phase-2-ibkr-spike.md](phase-2-ibkr-spike.md) | — | todo |
| 3. Provider trait + AV adapter | [phase-3-provider-trait.md](phase-3-provider-trait.md) | 1, 2 | todo |
| 4. IBKR provider implementation | [phase-4-ibkr-provider.md](phase-4-ibkr-provider.md) | 2, 3 | todo |
| 5. Cutover + AV deprecation | [phase-5-cutover.md](phase-5-cutover.md) | 4 | todo |

> **Status convention:** values are `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| Current AV service (becomes AV provider in Phase 3) | `src-tauri/src/services/financial_data_service/mod.rs` |
| AV per-endpoint fetchers (read-only reference) | `src-tauri/src/services/financial_data_service/{overview,income,earnings}.rs` |
| Stale-cache fallback pattern to mirror | `src-tauri/src/services/financial_data_service/news.rs:260-294` |
| MCP `get_fundamentals` tool (rewires in Phase 3) | `src-tauri/src/mcp/tools/fundamentals.rs` |
| Tracker fundamentals call site | `src-tauri/src/ibkr/commands/tracker.rs:42` |
| UI fundamentals command (also Phase 1 dedup) | `src-tauri/src/ibkr/commands/analysis.rs` |
| Frontend projections hook (Phase 1 dedup) | `src/features/analysis/hooks/useProjections.ts` |
| IBKR client trait + mock | `src-tauri/src/ibkr/client/`, `src-tauri/src/ibkr/mocks.rs` |
| Existing rate-limiter pattern | `src-tauri/src/middleware/historical_rate_limit.rs` |
| Cache service (TTL + read/write) | `src-tauri/src/services/cache_service.rs` |
| Service composition (provider wiring) | `src-tauri/src/lib.rs` |
| Settings (new `fundamentals_source` field) | `src-tauri/src/config/settings.rs` |
| FundamentalData type contract | `src-tauri/src/ibkr/types/fundamentals.rs` |

## Sequencing + cadence

- **W1:** Phase 1 (AV burn fixes — small, ships immediately) and Phase 2 (IBKR spike — independent) in parallel.
- **W2:** Phase 3 (provider trait — pure refactor, no behavior change). Gates Phases 4 and 5.
- **W3-4:** Phase 4 (IBKR provider implementation — bulk of the work).
- **W5+:** Phase 5 cutover. Soak ~2 weeks under shadow comparison before deleting AV provider.

Phase 1 and Phase 2 are independent — run in parallel if bandwidth allows. Phase 1's value is paid back even if the IBKR migration is later abandoned.

## Cross-phase verification

1. **Tracer-bullet test before Phase 5:** End-to-end. From a Claude Code session, ask `get_fundamentals(symbol="AAPL")` with `fundamentals_source = "ibkr"`. Expect identical-shape `FundamentalData` to what AV returned for the same symbol the same day. Shape mismatch means Phase 4 isn't done.
2. **Shadow comparison for Phase 5:** First 2 weeks after cutover, both providers run side-by-side on the same symbols (best-effort; AV may rate-limit). Diff parsed structures. Material disagreements (>5% on any numeric field) get logged to `QUESTIONS.md` with symbol, date, and field.
3. **Quota-budget invariant in CI:** Test that runs the morning sweep against a 100-ticker mock universe with `fundamentals_source = "ibkr"` and asserts zero AV HTTP requests fire (mock the AV transport to panic on call).
4. **No-mock-data invariant:** Test that calls each provider with a known-bad symbol; expects a typed error, not a silent mock-data fallback. Catches regressions of the `analysis.rs:53` mock fallback.
5. **`FundamentalData` shape parity:** Snapshot test that diff-asserts both providers return the same field set for AAPL.

## Open risks

- **`ibapi = "2"` may not expose `req_fundamental_data`.** Phase 2 spike resolves this. If the crate is missing it, options are: (a) upgrade or fork, (b) write a thin wrapper around the raw TWS message, (c) switch crates. Decision lives in Phase 2.
- **IBKR fundamentals require TWS up.** Background fetches die when the desktop app closes. Acceptable for v1 (single-user desktop), but blocks any "app-closed scheduled sweep" use case. The deferred Phase 9 daemon (prior roadmap) was the answer; flag in Phase 5 exit notes.
- **Reuters XML schema drift.** Reports change occasionally. Parser tests use real fixtures captured during Phase 2; refresh fixtures every ~6 months or when a parse error surfaces in production.
- **AV news still depends on `FinancialDataService`.** Phase 3 doesn't touch the news path. Don't accidentally delete the AV news code while removing AV fundamentals in Phase 5.
- **Settings flag drift.** `fundamentals_source` is the only place to switch providers. If a future phase introduces another fundamentals consumer that bypasses the trait, the flag stops being a kill switch. Code review owns this.
- **Subscription cost surprise.** Reuters Worldwide Fundamentals is ~$5–15/mo depending on residency. Confirm price + subscribe before starting Phase 2 smoke tests.
