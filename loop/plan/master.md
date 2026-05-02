# Alpha Vantage → IBKR: Full Vendor Strip-out

## Context

Alpha Vantage's free tier (25 calls/day, 1 req/sec) is the current source for company fundamentals (OVERVIEW, INCOME_STATEMENT, EARNINGS) and ticker-tagged news sentiment (NEWS_SENTIMENT). Logs from 2026-05-02 show the daily AV quota burning twice as fast as it should: the frontend `useProjections` hook calls `getFundamentalData` and `generateProjectionResults` in parallel, and the latter command internally re-fetches the same fundamentals — every UI projection lookup costs 6 AV requests instead of 3. There is also no in-flight coalescing across concurrent agent-loop callers, no per-vendor rate limiter, and no stale-cache fallback when AV returns its rate-limit `Information` payload (unlike news, which gracefully serves stale at `services/financial_data_service/news.rs:280-286`).

Even with all four bugs fixed, the free 25/day cap cannot carry the planned **100+ ticker** morning sweep. AV is on the agent-loop critical path: MCP `get_fundamentals` (`src-tauri/src/mcp/tools/fundamentals.rs:42`) and the tracker pipeline (`src-tauri/src/ibkr/commands/tracker.rs:42`) both reach it. Premium AV is $50/mo recurring; the user's IBKR account already supplies market data + news and can carry Reuters Worldwide Fundamentals (~$5–15/mo) plus the news subscriptions needed to retire AV from the news path too.

The goal: **delete Alpha Vantage from this codebase entirely.** Not just the fundamentals path. The `ALPHA_VANTAGE_API_KEY` env var, the `cache/alphavantage/` directory, the AV-specific rate limiter, the `FinancialDataService` struct, and both providers — all gone by the end. Two trait abstractions (`FundamentalsProvider`, `NewsProvider`) decouple call sites from vendor; IBKR is the only implementation that survives. Sentiment scoring, which AV provided per-article, becomes the responsibility of the existing `NewsInterpreter` LLM service.

## End-state architecture

Two trait abstractions, IBKR-only implementations behind both. Same downstream shapes (`FundamentalData`, `NewsItem`) so MCP tools, tracker, and UI never notice the swap. AV is deleted, not deprecated.

| Subsystem | Responsibility |
|---|---|
| **`FundamentalsProvider` trait** | Single async method `fetch(symbol) -> Result<FundamentalData, FundamentalsError>`. Mock-friendly. |
| **`IbkrFundamentalsProvider`** | Calls `req_fundamental_data` for ReportSnapshot + ReportsFinSummary + ReportsFinStatements + RESC, parses Reuters XML into `FundamentalData`. |
| **`NewsProvider` trait** | Async `fetch(symbol, lookback_hours) -> Result<Vec<NewsItem>, NewsError>`. Mock-friendly. Writes through to existing `news_cache` SQLite table. |
| **`IbkrNewsProvider`** | Calls IBKR news APIs (`req_historical_news` for backfill, `req_news_article` for body, optional `req_news_bulletins` for streaming) across configured news sources. |
| **`NewsInterpreter`** (unchanged) | Existing LLM service that adds `news_verdict_json` per cache row. Picks up the sentiment-scoring role AV used to fill. |
| **Caching** | Per-provider on-disk JSON cache (existing `CacheService`) for fundamentals, 7-day TTL. SQLite `news_cache` for news, unchanged TTL. |

## Hard invariants

1. **`FundamentalData` and `NewsItem` shapes are the contract.** Provider swaps don't change downstream code. Adding a field to either struct requires updating both providers in lockstep.
2. **Mock-friendly trait seams.** Both `FundamentalsProvider` and `NewsProvider` are `Send + Sync + 'static` and dyn-compatible; tests use `Fake*Provider`. The IBKR live impls are reached only via the existing `IbkrClientTrait` seam (`ibkr/mocks.rs`).
3. **Surveillance-only stays.** Neither fundamentals nor news work touches order placement. The MCP tool surface is unchanged — `get_fundamentals` and the news-consuming tools (e.g. `get_news`) stay the same; only the backend changes.
4. **Pre-commit sacred** — `cargo fmt --check`, `cargo clippy -D warnings`, `prettier`, `eslint`. Never `--no-verify`.
5. **No silent fallback to mock data on the migration path.** The existing `analysis.rs` "fall back to mock if AV fails" behavior is removed in Phase 3 — provider errors propagate as typed errors. Mock data exists for tests only.
6. **Final invariant after Phase 8: zero Alpha Vantage code.** `cargo build` does not link `FinancialDataService`; `rg -i "alpha.?vantage"` over `src-tauri/src` returns no production hits; `ALPHA_VANTAGE_API_KEY` does not appear in `.env.example` or `settings.rs`.

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **IBKR fundamentals subscription:** Reuters Worldwide Fundamentals via TWS Account → Market Data Subscriptions. Required from Phase 2 onward.
- **IBKR news subscriptions:** at minimum Reuters Real-time News (or equivalent regional bundle) so `req_historical_news` returns ticker-tagged items. Required from Phase 6 onward; specific subscription mix decided in Phase 6.
- **IBKR API crate:** existing `ibapi = "2"` (sync feature). Phase 2 confirmed `req_fundamental_data` is missing — fork plan recorded. Phase 6 will likely repeat that finding for news APIs.
- **Cache TTL:** 7 days per provider for fundamentals (unchanged); existing `news_cache` TTL retained.
- **Settings flags during migration:** `fundamentals_source: "ibkr" | "alpha_vantage"` (Phases 3–5), `news_source: "ibkr" | "alpha_vantage"` (Phases 7–8). Both flags **and the AV adapter code** are deleted in Phase 8.
- **Sentiment replacement:** AV's per-article sentiment scores are not replicated. `NewsInterpreter` produces a per-symbol verdict that downstream consumers already use; per-article scores are lost. Acceptable trade-off, revisit only if a consumer needs them.

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. AV burn fixes (stop the bleed) | [phase-1-av-burn-fixes.md](phase-1-av-burn-fixes.md) | — | done (commit c239bf4, 2026-05-02) |
| 2. IBKR Reuters spike (de-risk) | [phase-2-ibkr-spike.md](phase-2-ibkr-spike.md) | — | in-progress (started 2026-05-02) |
| 3. Fundamentals provider trait + AV adapter | [phase-3-provider-trait.md](phase-3-provider-trait.md) | 1, 2 | todo |
| 4. IBKR fundamentals provider implementation | [phase-4-ibkr-provider.md](phase-4-ibkr-provider.md) | 2, 3 | todo |
| 5. Fundamentals cutover + AV-fundamentals deprecation | [phase-5-cutover.md](phase-5-cutover.md) | 4 | todo |
| 6. IBKR news spike (de-risk) | [phase-6-ibkr-news-spike.md](phase-6-ibkr-news-spike.md) | — | todo |
| 7. News provider trait + IBKR news provider | [phase-7-news-provider.md](phase-7-news-provider.md) | 3, 6 | todo |
| 8. Full AV deletion (news cutover + module rip-out) | [phase-8-av-deletion.md](phase-8-av-deletion.md) | 5, 7 | todo |

> **Status convention:** values are `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| Current AV service (becomes AV provider in Phase 3, deleted in Phase 8) | `src-tauri/src/services/financial_data_service/mod.rs` |
| AV per-endpoint fetchers (read-only reference; deleted in Phase 5) | `src-tauri/src/services/financial_data_service/{overview,income,earnings}.rs` |
| AV news fetcher (becomes AV news provider in Phase 7, deleted in Phase 8) | `src-tauri/src/services/financial_data_service/news.rs` |
| Stale-cache fallback pattern to mirror | `src-tauri/src/services/financial_data_service/news.rs:260-294` |
| MCP `get_fundamentals` tool (rewires in Phase 3) | `src-tauri/src/mcp/tools/fundamentals.rs` |
| MCP news-consuming tools (rewire in Phase 7) | `src-tauri/src/mcp/tools/news.rs` |
| Tracker fundamentals + news call sites | `src-tauri/src/ibkr/commands/tracker.rs:42`, `src-tauri/src/services/tracker_runner/` |
| News interpreter (sentiment scoring after AV is gone) | `src-tauri/src/services/news_interpreter/mod.rs` |
| UI fundamentals command (also Phase 1 dedup) | `src-tauri/src/ibkr/commands/analysis.rs` |
| Frontend projections hook (Phase 1 dedup) | `src/features/analysis/hooks/useProjections.ts` |
| IBKR client trait + mock | `src-tauri/src/ibkr/client/`, `src-tauri/src/ibkr/mocks.rs` |
| AV rate limiter (Phase 1 add, Phase 8 delete) | `src-tauri/src/middleware/alpha_vantage_rate_limit.rs` |
| Existing rate-limiter pattern | `src-tauri/src/middleware/historical_rate_limit.rs` |
| Cache service (TTL + read/write; AV cache dir deleted in Phase 8) | `src-tauri/src/services/cache_service.rs` |
| Service composition (provider wiring) | `src-tauri/src/lib.rs` |
| Settings (`fundamentals_source` Phase 3, `news_source` Phase 7, both gone Phase 8) | `src-tauri/src/config/settings.rs` |
| `FundamentalData` type contract | `src-tauri/src/ibkr/types/fundamentals.rs` |
| `NewsItem` type contract | `src-tauri/src/ibkr/types/news.rs` |
| News cache (SQLite — backend stays, producer changes) | `src-tauri/migrations/V*.sql` (`news_cache` table) |
| `ALPHA_VANTAGE_API_KEY` references (4 sites, all deleted Phase 8) | `src-tauri/src/{config/settings.rs:319, lib.rs:116, services/financial_data_service/mod.rs:499, services/financial_data_service/news.rs:260}` |

## Sequencing + cadence

- **W1:** Phase 1 (AV burn fixes — done) and Phase 2 (IBKR fundamentals spike — in progress) in parallel.
- **W2:** Phase 3 (provider trait — pure refactor). Gates Phases 4 + 5.
- **W3-4:** Phase 4 (IBKR fundamentals provider).
- **W5-6:** Phase 5 cutover. Soak ~2 weeks under shadow comparison before deleting AV fundamentals adapter.
- **W5-6 (parallel):** Phase 6 (IBKR news spike — independent of fundamentals migration; can start as soon as Phase 1 is done).
- **W7-8:** Phase 7 (news provider trait + IBKR news provider).
- **W9+:** Phase 8 (news cutover + final AV deletion). Soak ~2 weeks under shadow comparison before the deletion commit.

Phase 6 is independent of Phases 2–5 and can run in parallel as soon as bandwidth allows. Phase 7 depends on Phase 3 only because it mirrors the trait pattern Phase 3 establishes — start it earlier if Phase 3 lands first. Phase 8 is the only phase that requires both migration arcs to be soaked-and-clean.

## Cross-phase verification

1. **Tracer-bullet test before Phase 5:** End-to-end. From a Claude Code session, ask `get_fundamentals(symbol="AAPL")` with `fundamentals_source = "ibkr"`. Expect identical-shape `FundamentalData` to what AV returned for the same symbol the same day. Shape mismatch means Phase 4 isn't done.
2. **Tracer-bullet test before Phase 8:** End-to-end. From a Claude Code session, ask the news-consuming MCP tool for AAPL with `news_source = "ibkr"`. Expect non-empty `NewsItem[]` whose top-level fields match what AV returned for the same lookback window the same day. `NewsInterpreter` produces a verdict from the IBKR-sourced cache row.
3. **Shadow comparison for Phase 5 (fundamentals):** First 2 weeks after cutover, both providers run side-by-side on the same symbols (best-effort; AV may rate-limit). Diff parsed structures. Material disagreements (>5% on any numeric field) get logged to `QUESTIONS.md` with symbol, date, and field.
4. **Shadow comparison for Phase 7 (news):** First 2 weeks after `news_source` flips to IBKR, AV news fetch runs in parallel and the two `NewsItem[]` lists are diffed by `(symbol, time_published)`. Coverage gaps logged to `QUESTIONS.md`. Sentiment-score loss is acknowledged, not flagged.
5. **Quota-budget invariant in CI:** Test that runs the morning sweep against a 100-ticker mock universe with both sources set to `"ibkr"` and asserts zero AV HTTP requests fire (mock the AV transport to panic on call).
6. **No-mock-data invariant:** Test that calls each provider with a known-bad symbol; expects a typed error, not a silent mock-data fallback. Catches regressions of the `analysis.rs:53` mock fallback.
7. **Shape parity:** Snapshot test that diff-asserts both providers return the same field set for AAPL (separately for fundamentals and for news).
8. **Final AV-elimination check (Phase 8 exit):** CI test that greps `src-tauri/src` for `alpha_vantage`, `AlphaVantage`, `ALPHA_VANTAGE_API_KEY`. Build fails if any production reference survives. Allowed: comments in CHANGELOG / migration notes; nothing in source.

## Open risks

- **`ibapi = "2"` may not expose `req_fundamental_data` or `req_historical_news`.** Phase 2 confirmed the fundamentals gap (fork plan in place). Phase 6 will likely confirm the news gap; the same fork extends to cover both.
- **IBKR fundamentals + news both require TWS up.** Background fetches die when the desktop app closes. Acceptable for v1 (single-user desktop), but blocks any "app-closed scheduled sweep" use case. The deferred Phase 9 daemon (prior roadmap) was the answer; flag in Phase 8 exit notes.
- **Reuters XML schema drift.** Reports change occasionally. Parser tests use real fixtures captured during Phase 2; refresh fixtures every ~6 months or when a parse error surfaces in production.
- **News sentiment quality.** AV ships per-article sentiment scores. `NewsInterpreter` produces a per-symbol verdict only. Downstream consumers that needed article-level scoring will lose data. Phase 7 audits all `NewsItem` consumers and either tolerates the loss or raises a dedicated sub-task.
- **News coverage gap.** Different IBKR news subscriptions return different sources; AV's NEWS_SENTIMENT was a single feed. Phase 6 must verify coverage for representative symbols (large-cap, mid-cap, small-cap, ADR) before declaring a viable replacement.
- **Settings flag drift.** `fundamentals_source` and `news_source` are the only places to switch providers. Both flags vanish in Phase 8 along with the AV code; any future fundamentals or news consumer must go through the trait.
- **Subscription cost surprise.** Reuters Worldwide Fundamentals is ~$5–15/mo; IBKR news subs vary. Confirm price + subscribe before starting Phase 2 / Phase 6 smoke tests.
- **`ALPHA_VANTAGE_API_KEY` removal will break dev environments that still have it set.** Harmless, but deserves a CHANGELOG note in the Phase 8 deletion commit.
