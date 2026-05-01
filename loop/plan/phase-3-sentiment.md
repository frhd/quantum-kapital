# Phase 3 — Social sentiment ingestion (Reddit + Stocktwits + Apewisdom)

> Part of [Quantum Kapital → Autonomous Researcher](master.md). See index for invariants.

**Status:** done (commit 0bd8511, 2026-05-02)

**Depends on:** Phase 1 (MCP read tools — adds `get_sentiment`)

**Goal:** Continuous-ingestion services for three sentiment sources. New `social_sentiment` table. New scheduler. New MCP read tool. No X.com in v1 (deferred — see index defaults).

## Files

- New service dir: `src-tauri/src/services/social_sentiment/`
  - `mod.rs` — orchestrator, fans out to providers in parallel
  - `reddit.rs` — `roux` crate or Python sidecar (PRAW). Decide in spike.
  - `stocktwits.rs` — free trending endpoint
  - `apewisdom.rs` — free aggregator API for WSB sentiment scores
  - `ticker_filter.rs` — validity filter, kills false positives like `$A` / `$YOU` / `$IT`
- New scheduler: `src-tauri/src/services/social_sentiment_scheduler/mod.rs` (60-min cadence, `Clock` enum pattern from `eod_scheduler`)
- New migration: `social_sentiment(id, source, symbol, score, mentions_24h, sentiment_label, raw_payload JSON, fetched_at)`
- New MCP tool in `mcp/tools/reads.rs`: `get_sentiment(symbol, since, sources?)`
- Touches: `src-tauri/src/config/settings.rs` — `SocialSentimentConfig` with API keys (Reddit client_id/secret), rate-limit settings, source enable flags
- Touches: `src-tauri/.env.example` — document new env vars

## Reuse

- `CacheService` pattern for raw payload caching.
- `LlmService` (Haiku) for sentiment scoring of unstructured text where aggregator scores are missing.
- Scheduler pattern from `eod_scheduler` / `intraday_scheduler` (Clock enum, RTH-window gating, last-run-date dedup).
- News interpreter pattern from `services/news_interpreter/` for structured-output sentiment classification if needed.

## Decisions to make in this phase

- **Reddit auth: Rust (`roux`) vs Python sidecar (PRAW).** PRAW is much more battle-tested; Rust crates for Reddit OAuth tend to be flaky. Spike on day 1; default to Python sidecar if `roux` looks rough.
- **Ticker validity universe.** Need a known-symbol set for the filter. Options: IBKR symbol search (rate-limited), static NASDAQ/NYSE list (stale), or the union of `tracked_tickers` + a top-2000 list. Start with static + dynamic union.
- **Cadence.** 60min default; high-velocity tickers may want 15min during RTH. Make configurable per-source.

## Exit criteria

- `get_sentiment("TSLA", since=24h)` returns recent WSB mention count, Stocktwits sentiment, Apewisdom rank/score in a single response.
- New UI sentiment widget on the analysis view (small — one row, three sources, last-updated timestamp).
- Ticker filter rejects `$A`, `$TO`, `$YOU` from raw text on a fixture test.
- Scheduler runs at the configured cadence; raw payloads cached; scoring/labeling persisted.

## Gotchas

- **Reddit OAuth refresh.** Long-running refresh tokens can expire silently. Add a healthcheck and surface auth failures in UI.
- **Rate limits.** Reddit: 60 req/min authed. Stocktwits free: 200/hr. Apewisdom: undocumented but generous. Build per-source rate limiter (mirror `HistoricalRateLimiter` shape).
- **Sentiment polarity drift.** Aggregator "score" semantics differ across sources. Normalize to `[-1, 1]` at the service layer; preserve raw in payload.
- **Quiet days.** If an API is down or returns nothing, persist a `null`-score row marked stale rather than gap. Helps the agent know "we tried and there's no signal" vs "we never asked."
