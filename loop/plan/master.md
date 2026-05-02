# Alpha Vantage strip-out: manual MCP fundamentals + IBKR news (AV → opportunistic fallback)

## Context

Original plan (commits `bdd0906` through `047dc59`): replace Alpha Vantage with IBKR Reuters Worldwide Fundamentals via `req_fundamental_data` and IBKR news via `req_historical_news`. Phase 2 (IBKR fundamentals spike) hit two compounding blockers in 2026-05-02:

1. **The `reqFundamentalData` API is officially DEPRECATED** in IBKR's TWS API docs ("this interface still works as of now, but it is possible that IB will stop honoring these requests in the future"). Throughout 2024-2025, community evidence (`twsapi@groups.io`, `quantbelt/ib_fundamental` discussions) shows the API degrading in place even for entitled accounts — `ReportsFinStatements` broke for many users after the March 2025 TWS upgrade and stays broken.
2. **This account does not have the API entitlement** despite IBIS Research Platform being active (which feeds the TWS UI Financials tab). Test call returned **error 10358 "Fundamentals data is not allowed"** for all four reportTypes. The historical "Reuters Worldwide Fundamentals" GFIS line item does not appear on the user's subscriptions page; IBKR has been quietly winding down API entitlements for retail.

Building Phase 4's XML parsers around a deprecated, degrading API on an account that can't reach it would be building on sand. **Pivot decided 2026-05-02: drop the IBKR fundamentals migration; replace it with a manual-paste path via a new MCP write tool.** Phase 1's burn fixes already brought AV's quota under control; AV stays as an opportunistic fallback for fundamentals (with hard guardrails). The news arc (Phases 6/7) is unaffected — IBKR news APIs are still live, supported, and present in the published `ibapi = "2.11.x"` crate (Phase 6 confirmed: no fork needed for news).

**The investigation (`grep` on tracker pipeline + strategies, 2026-05-02) confirmed the tracker does not read fundamentals at all.** `StrategyContext.fundamentals` exists as an `Option<>` field but no production strategy reads it. The only fundamentals consumers are user-explicit: the analysis/projection Tauri command and the MCP `get_fundamentals` tool. AV's 25/day cap is therefore not under sweep pressure — the only callers are the user opening UIs or asking Claude.

**Outcome.** AV is reduced to an opportunistic per-symbol fallback for fundamentals only, never touched by background work. Manual paste via MCP becomes the primary path for tickers the user is actually researching. Every IBKR news consumer migrates as originally planned. The `ALPHA_VANTAGE_API_KEY` env var, the AV news fetcher, and the AV cache directory survive only on the fundamentals fallback path; the news side is fully deleted.

## End-state architecture

| Subsystem | Responsibility |
|---|---|
| **`FundamentalsProvider` trait** | Single async `fetch(symbol) -> Result<FundamentalData, FundamentalsError>`. Mock-friendly. Production impl is `CompositeFundamentalsProvider`. |
| **`CompositeFundamentalsProvider`** | Reads in order: (1) manual store, (2) AV cache (fresh, then stale on rate-limit), (3) AV API call (only if budget allows + not rate-limited recently). |
| **`ManualFundamentalsStore`** | SQLite-backed key-value store keyed by symbol; rows written by the MCP `set_fundamentals` tool. Each row carries `as_of_date`, `source`, and a JSON-encoded `FundamentalData`. |
| **`AlphaVantageFundamentalsProvider`** | Surviving wrapper around the existing `FinancialDataService::fetch_fundamental_data`. Guarded: per-symbol per-day cap, daily ledger soft-cap at 20/25, hard-cap at 25, manual-write invalidates the AV cache row. |
| **MCP `set_fundamentals` tool** | New write tool. LLM submits parsed fundamentals; server validates against `FundamentalData` JSON schema; persists to `ManualFundamentalsStore`; invalidates AV cache for that symbol; audited via `services/mcp_audit/` (same rail as `ack_alert`). |
| **`NewsProvider` trait** | Async `fetch(symbol, lookback_hours) -> Result<Vec<NewsItem>, NewsError>`. Mock-friendly. Writes through to existing `news_cache` SQLite table. |
| **`IbkrNewsProvider`** | Calls IBKR news APIs (`req_historical_news` + `req_news_article` + `req_news_providers`) across configured news subscriptions. Phase 6 confirmed: published `ibapi = "2.11.x"` already exposes these methods — no fork needed for news. |
| **`NewsInterpreter`** (unchanged) | Existing LLM service that adds `news_verdict_json` per cache row. Picks up the per-symbol sentiment role AV used to fill at the per-article level. |
| **Caching** | File-based `CacheService` for AV (7-day TTL, retained behind the AV provider). SQLite `news_cache` for news, unchanged. New `manual_fundamentals` SQLite table for the manual store, no TTL (data is asof-tagged). |

## Hard invariants

1. **`FundamentalData` and `NewsItem` shapes are the contract.** Provider swaps don't change downstream code. Adding a field to either struct requires updating both providers in lockstep.
2. **Mock-friendly trait seams.** Both `FundamentalsProvider` and `NewsProvider` are `Send + Sync + 'static` and dyn-compatible. Tests use `Fake*Provider`. The IBKR live impls are reached only via the existing `IbkrClientTrait` seam (`ibkr/mocks.rs`).
3. **Surveillance-only stays.** Neither fundamentals nor news work touches order placement. The new MCP `set_fundamentals` tool is a write but adheres to the surveillance contract (it writes operator-curated data, not market actions). Audit through `services/mcp_audit/`.
4. **Pre-commit sacred** — `cargo fmt --check`, `cargo clippy -D warnings`, `prettier`, `eslint`. Never `--no-verify`.
5. **No silent fallback to mock data.** The existing `analysis.rs` "fall back to mock if AV fails" behavior is removed in Phase 3 — provider errors propagate as typed errors. Mock data exists for tests only.
6. **Tracker MUST NOT fetch fundamentals.** `tracker_runner` and every strategy compile without referencing `FundamentalsProvider`. A regression test asserts this. (Currently true — keep it that way.)
7. **AV is opportunistic-only.** The AV branch of `CompositeFundamentalsProvider` is reached on user-explicit fetches only, never by background sweeps or schedulers. The daily-AV-call ledger is the safety net; if anything ever calls AV more than the per-day cap allows, an alert is emitted.
8. **MCP write invalidates AV cache.** Once `set_fundamentals` lands a row for symbol X, the AV cache row for X is purged and the AV provider returns `NotFound` for X (manual store wins forever, until cleared explicitly).
9. **News-side AV is fully deleted in Phase 8.** Hard invariant #6 from the prior plan ("zero AV code") is **relaxed** for fundamentals (AV adapter retained as fallback) and **kept** for news (AV news code, env var dependency on the news side, cache directory news entries — all gone).

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **Manual store schema:** SQLite table `manual_fundamentals(symbol PRIMARY KEY, as_of_date TEXT, source TEXT, payload_json TEXT, written_at TEXT, written_by TEXT)`. New refinery migration in Phase 4.
- **MCP tool input schema:** strict JSON Schema generated from `FundamentalData` plus required envelope fields (`symbol`, `as_of_date`, `source`). Server returns clear validation errors so the LLM can self-correct.
- **AV daily ledger:** soft cap at 20 calls/day, hard cap at 25 calls/day. Tracked in a new `av_call_ledger(date PRIMARY KEY, count INTEGER)` SQLite table or in-memory `Mutex<HashMap<NaiveDate, u32>>` (decided in Phase 5).
- **AV per-symbol cap:** 1 fetch per symbol per day. Subsequent fetches return cached value (stale OK) without hitting AV.
- **IBKR news subscriptions:** at minimum Reuters Real-time News (or equivalent regional bundle). Phase 6 in-progress; specific subscription mix recorded in `QUESTIONS.md § P3` once user confirms.
- **IBKR news API crate:** published `ibapi = "2.11.x"` — already exposes `news_providers`, `historical_news`, `news_article`. No fork. Phase 6 confirmed.
- **Cache TTL:** 7 days for AV file cache (unchanged); manual store has no TTL but is asof-tagged.
- **Settings flag during migration:** `news_source: "ibkr" | "alpha_vantage"` (Phases 7–8). Both flag and the AV news adapter are deleted in Phase 8. **No `fundamentals_source` flag** — the composite provider is the only path; runtime behavior is determined by what's in the manual store + AV cache.
- **Sentiment replacement:** AV's per-article sentiment scores are not replicated. `NewsInterpreter` produces a per-symbol verdict that downstream consumers already use; per-article scores are lost. Acceptable trade-off (Phase 6 audit confirmed every consumer tolerates this).

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. AV burn fixes (stop the bleed) | [phase-1-av-burn-fixes.md](phase-1-av-burn-fixes.md) | — | done (commit c239bf4, 2026-05-02) |
| 2. IBKR Reuters spike (de-risk) | [phase-2-ibkr-spike.md](phase-2-ibkr-spike.md) | — | abandoned (2026-05-02 — API deprecated + 10358 entitlement gap; see master.md Context) |
| 3. Fundamentals provider trait + AV adapter | [phase-3-provider-trait.md](phase-3-provider-trait.md) | 1 | done (commit bd6e835, 2026-05-02) |
| 4. MCP `set_fundamentals` write tool + manual store + composite provider | [phase-4-mcp-fundamentals.md](phase-4-mcp-fundamentals.md) | 3 | done (commit ab87c5d, 2026-05-02) |
| 5. Cutover: composite default + AV guardrails (daily cap + per-symbol cap + tracker invariant test) | [phase-5-cutover.md](phase-5-cutover.md) | 4 | in-progress (started 2026-05-02) |
| 6. IBKR news spike (de-risk) | [phase-6-ibkr-news-spike.md](phase-6-ibkr-news-spike.md) | — | in-progress (started 2026-05-02) |
| 7. News provider trait + IBKR news provider | [phase-7-news-provider.md](phase-7-news-provider.md) | 3, 6 | todo |
| 8. AV news deletion (fundamentals AV adapter retained as fallback) | [phase-8-av-deletion.md](phase-8-av-deletion.md) | 5, 7 | todo |

> **Status convention:** values are `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)` | `abandoned (YYYY-MM-DD — reason)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| Current AV service (becomes AV provider in Phase 3, **retained** as fundamentals fallback after Phase 8) | `src-tauri/src/services/financial_data_service/mod.rs` |
| AV per-endpoint fetchers (read-only reference; **retained** for fundamentals fallback) | `src-tauri/src/services/financial_data_service/{overview,income,earnings}.rs` |
| AV news fetcher (becomes AV news provider in Phase 7, **deleted** in Phase 8) | `src-tauri/src/services/financial_data_service/news.rs` |
| Stale-cache fallback pattern to mirror | `src-tauri/src/services/financial_data_service/news.rs:260-294` |
| MCP `get_fundamentals` tool (rewires in Phase 3 to read composite provider) | `src-tauri/src/mcp/tools/fundamentals.rs` |
| MCP `set_fundamentals` tool (NEW in Phase 4) | `src-tauri/src/mcp/tools/set_fundamentals.rs` |
| MCP audit rail (extend in Phase 4 to cover `set_fundamentals`) | `src-tauri/src/services/mcp_audit/` |
| MCP news-consuming tools (rewire in Phase 7) | `src-tauri/src/mcp/tools/news.rs` |
| Tracker (must NOT depend on FundamentalsProvider — invariant) | `src-tauri/src/services/tracker_runner/`, `src-tauri/src/strategies/` |
| News interpreter (sentiment scoring after AV news is gone) | `src-tauri/src/services/news_interpreter/mod.rs` |
| UI fundamentals command (Phase 3 rewires through trait) | `src-tauri/src/ibkr/commands/analysis.rs` |
| Frontend projections hook (Phase 1 dedup — done) | `src/features/analysis/hooks/useProjections.ts` |
| IBKR client trait + mock | `src-tauri/src/ibkr/client/`, `src-tauri/src/ibkr/mocks.rs` |
| AV rate limiter (Phase 1 add — kept as long as AV adapter survives) | `src-tauri/src/middleware/alpha_vantage_rate_limit.rs` |
| Existing rate-limiter pattern | `src-tauri/src/middleware/historical_rate_limit.rs` |
| Cache service (TTL + read/write; AV cache directory **kept** for fundamentals fallback) | `src-tauri/src/services/cache_service.rs` |
| Service composition (provider wiring) | `src-tauri/src/lib.rs` |
| Settings (`news_source` Phase 7, gone Phase 8; **no `fundamentals_source` flag** in this plan) | `src-tauri/src/config/settings.rs` |
| `FundamentalData` type contract | `src-tauri/src/ibkr/types/fundamentals.rs` |
| `NewsItem` type contract | `src-tauri/src/ibkr/types/news.rs` |
| News cache (SQLite — backend stays, producer changes) | `src-tauri/migrations/V*.sql` (`news_cache` table) |
| New SQLite tables (Phases 4 + 5) | `manual_fundamentals`, `av_call_ledger` (or in-memory equivalent) |
| `ALPHA_VANTAGE_API_KEY` references (deleted from news-side sites in Phase 8; **kept** on fundamentals-side sites) | `src-tauri/src/config/settings.rs:319`, `src-tauri/src/lib.rs:116`, `src-tauri/src/services/financial_data_service/{mod.rs:499, news.rs:260}` |

## Sequencing + cadence

- **W1:** Phase 1 (AV burn fixes — done) and Phase 2 (IBKR fundamentals spike — abandoned 2026-05-02; outcome captured) and Phase 6 (IBKR news spike — in progress) in parallel.
- **W2:** Phase 3 (provider trait — pure refactor). Gates Phase 4.
- **W3:** Phase 4 (MCP `set_fundamentals` tool + manual store + composite provider).
- **W4:** Phase 5 cutover (composite default + AV guardrails + tracker invariant test).
- **W4-5 (parallel):** Phase 7 (news provider trait + IBKR news provider) — depends on Phase 3 (trait pattern) and Phase 6.
- **W6:** Phase 8 (news cutover + AV news deletion). Soak ~2 weeks under shadow comparison before the deletion commit.

The fundamentals arc shrinks dramatically: from "fork ibapi + write XML parsers + integration tests" (~3 weeks) to "design MCP write tool + ManualFundamentalsStore + composite provider + cutover" (~1 week). Phase 6 is independent of fundamentals work and can run in parallel as soon as the user is at the desk with TWS up. Phase 8 is the only phase that requires both fundamentals (composite-default) and news (IBKR cutover) to be soaked-and-clean.

## Cross-phase verification

1. **Tracer-bullet test before Phase 5:** End-to-end. From a Claude Code session, call `set_fundamentals(symbol="AAPL", as_of_date="...", source="...", current_metrics={...})`, then `get_fundamentals(symbol="AAPL")`. The returned `FundamentalData` matches the submitted payload. Subsequent `get_fundamentals` calls do not hit AV (verified via call-ledger inspection).
2. **Tracer-bullet test before Phase 8:** End-to-end. From a Claude Code session, ask the news-consuming MCP tool for AAPL with `news_source = "ibkr"`. Expect non-empty `NewsItem[]` whose top-level fields match what AV returned for the same lookback window the same day. `NewsInterpreter` produces a verdict from the IBKR-sourced cache row.
3. **AV-budget invariant in CI:** Test that runs the tracker against a 100-ticker mock universe and asserts zero AV HTTP requests fire (mock the AV transport to panic on call). Catches accidental AV reads from background code.
4. **Tracker-doesn't-read-fundamentals invariant:** Compile-time check that `tracker_runner` and `strategies/` modules do not import `FundamentalsProvider` or `FundamentalData`. Implemented as a `cargo test` that greps the dep graph (or a `#[deny]` attribute on a sentinel symbol the tracker would have to mention to use it).
5. **Manual-write-invalidates-AV-cache test:** call `set_fundamentals(symbol="X")`; assert AV cache row for X is gone and a subsequent provider fetch returns the manual data without consulting AV.
6. **AV daily-cap test:** With ledger pre-populated to 25/25, call the AV provider for an unseen symbol; assert `FundamentalsError::DailyBudgetExhausted`. With ledger at 24/25 (within soft cap of 20), assert a clear warn-log but the call succeeds.
7. **No-mock-data invariant:** Test that calls each provider with a known-bad symbol; expects a typed error, not a silent mock-data fallback. Catches regressions of the `analysis.rs:53` mock fallback.
8. **Final AV-news-elimination check (Phase 8 exit):** CI test that greps `src-tauri/src/services/financial_data_service/news.rs` and the news-side env-var sites — file should be gone, `news_source` flag gone. AV fundamentals adapter (`mod.rs`, `overview.rs`, `income.rs`, `earnings.rs`) should still exist (build links it for the fallback path).

## Open risks

- **AV adapter rot.** The AV fundamentals adapter is now load-bearing for the fallback. If AV deprecates the free tier or further restricts endpoints, the fallback breaks silently for symbols not in the manual store. Mitigation: monitor failures via the AV call ledger; consider a periodic synthetic test that calls `get_fundamentals` for a control symbol and alerts on persistent failures.
- **Manual coverage decay.** A symbol the user researched 6 months ago has stale fundamentals in the manual store. Mitigation: every manual row has `as_of_date`; the analysis UI banner-warns when data is >90 days old; MCP `get_fundamentals` returns the date so the LLM can reason about freshness.
- **LLM mis-extraction.** The LLM parses pasted text into structured JSON; it can hallucinate or transpose. Mitigations: strict JSON Schema validation (catches type errors), range/sanity checks (P/E > 0, market_cap reasonable), diff-against-prior in the tool response (5x changes flagged), LLM instruction to refuse if unclear, explicit `source` field forces provenance disclosure.
- **MCP write tool surface.** Adding a write tool is a meaningful surface expansion. Mitigations: audited via `services/mcp_audit/` (same rail as `ack_alert`); never autonomously called by background processes; explicit invariant that no order tools are added under cover of this expansion.
- **IBKR news + TWS up.** News fetching requires TWS running. Acceptable for v1 (single-user desktop). The deferred Phase 9 daemon (prior roadmap) is the long-term answer; flag in Phase 8 exit notes.
- **News coverage gap.** Different IBKR news subscriptions return different sources; AV's NEWS_SENTIMENT was a single feed. Phase 6 must verify coverage for representative symbols (large-cap, mid-cap, small-cap, ADR) before declaring a viable replacement.
- **Settings flag drift.** `news_source` is the only place to switch news providers and vanishes in Phase 8 along with the AV news code; any future news consumer must go through the trait.
- **`ALPHA_VANTAGE_API_KEY` partial dependency.** After Phase 8, the env var is read only by the surviving fundamentals AV adapter. If the user unsets it, fallback breaks (manual store still works). Document loudly in the Phase 8 commit body and `CLAUDE.md` updates.
