# Phase 6 — IBKR news spike (de-risk)

> Part of [Alpha Vantage → IBKR: Full Vendor Strip-out](master.md). See index for invariants.

**Status:** done (fixtures captured 2026-05-02 against paper Gateway port 4004 — 8 providers, 50 AAPL headlines / 24h, 1 Text-type article body)

**Depends on:** none (independent of Phases 2–5; can run in parallel)

**Goal:** Confirm IBKR's news APIs are reachable from this codebase, decide which sources to consume, capture sample payloads, and verify that `NewsInterpreter`-only sentiment scoring is acceptable in place of AV's per-article scores. Outcomes mirror Phase 2 for the news side: (a) crate-path decision (likely the same `ibapi` fork extended to cover news), (b) subscription confirmation, (c) on-disk fixtures for the JSON / structured-news payloads, (d) explicit "sentiment loss is acceptable" call recorded after auditing downstream consumers.

## Files

- New: `src-tauri/tests/fixtures/ibkr_news/AAPL_historical.json` — output of `req_historical_news` for AAPL over a 24h window across all configured sources.
- New: `src-tauri/tests/fixtures/ibkr_news/AAPL_article_<headline_id>.json` — output of `req_news_article` for one item from the historical list (article body / structured payload).
- New: `src-tauri/tests/fixtures/ibkr_news/news_providers.json` — output of `req_news_providers` (list of subscribed news sources for the connected account).
- New: `src-tauri/src/bin/ibkr_news_spike.rs` — throwaway binary mirroring `ibkr_fundamentals_spike.rs`. Connects to TWS, calls `req_news_providers`, then `req_historical_news` for AAPL across all subscribed providers, then `req_news_article` for one item. Writes the captures to the fixture paths above. Deleted at end of phase or moved behind `#[cfg(feature = "ibkr-spike")]`.
- New: `loop/plan/notes/ibkr-news-shape.md` — notes on the structured news payload (top-level fields, where headline / body / source / time live, ticker-tagging behavior across providers, any noted differences vs. AV's `NewsItem` shape).
- New: `loop/plan/notes/sentiment-loss-audit.md` — list of every code path that currently reads AV's per-article sentiment fields (`overall_sentiment_score`, `ticker_sentiment[]`, etc.). For each, record what would change if those fields were `None`. Decision: tolerate or build a substitute.

## Reuse

- Existing `IbkrClient` connection plumbing (the Phase 2 spike binary established this).
- Existing TWS connection settings in `src-tauri/src/config/settings.rs`.
- The `ibapi` fork started in Phase 2 — extend it here rather than starting a new fork.
- Existing `NewsItem` type (`src-tauri/src/ibkr/types/news.rs`) and `news_cache` SQLite table — these stay; only the producer changes.

## Decisions to make in this phase

- **News sources to subscribe to.** Subscribed providers vary by region and account tier. Default mix to verify: Reuters Real-time News (broad coverage), Briefing.com (US-focused), Dow Jones (premium US). Document which the user has, which to add, which to skip. Cost confirmation needed. **Pending user confirmation** — see `QUESTIONS.md § P3`. Resolved when the user runs the spike capture and pastes the resulting `news_providers.json` provider list back into `QUESTIONS.md`.
- **Crate path.** **DECIDED 2026-05-02 — use the released `ibapi = "2.11.x"` directly. No fork needed for news.** Verified against the locally-resolved 2.11.2 source: `Client::news_providers` (`client/sync.rs:1764`), `Client::historical_news` (`client/sync.rs:1831`), `Client::news_article` (`client/sync.rs:1863`), and `Client::news_bulletins` (`client/sync.rs:1786`) are all public on the sync feature. Outgoing-message IDs 84/85/86 and incoming-message IDs 83/85/86/87 are present in `messages.rs:584-592` / `messages.rs:232-241`; server-version gates 115/116/117 are in `server_versions.rs`. This is the OPPOSITE of Phase 2's fundamentals decision — the news APIs are stable and released. Phase 2's fork stays orthogonal and unchanged. Full investigation in `loop/plan/notes/ibkr-news-shape.md`.
- **Streaming vs. polling.** **DECIDED 2026-05-02 — poll-only via `historical_news` for v1.** AV NEWS_SENTIMENT was poll-only; matching that cadence keeps Phase 7 a producer-swap rather than a behavioural change. `news_bulletins` is exposed but explicitly out of scope for v1.
- **Sentiment-loss tolerance.** **DECIDED 2026-05-02 — tolerate.** Audit complete in `loop/plan/notes/sentiment-loss-audit.md`: every consumer of AV's per-article sentiment fields (5 production sites, 6 test sites) resolves to graceful degradation. The `NewsInterpreter` per-symbol verdict path is the primary signal and is unaffected by the producer swap. Phase 8 will land a regression test asserting the IBKR producer fills these fields with `None` / `Vec::new()`.
- **Lookback window.** **DECIDED 2026-05-02 — `lookback_hours → start = now - lookback_hours, end = now, total_results = 50`.** Mirrors AV's `limit=50`. Recorded in `ibkr-news-shape.md` § "Lookback-window mapping".

## Exit criteria

- Three fixtures exist on disk, all non-empty and parseable.
- Spike binary runs to completion against a paper or live TWS account from a clean checkout: `cargo run --bin ibkr_news_spike --features ibkr-spike`.
- `loop/plan/notes/ibkr-news-shape.md` documents: top-level structure of historical news vs. article payloads, ticker-tagging behavior (does each item carry the queried symbol explicitly, or only via headline parsing?), source-string format, time format and time zone.
- `loop/plan/notes/sentiment-loss-audit.md` enumerates every consumer of AV per-article sentiment fields with file:line, and for each records "tolerate" or "needs substitute".
- Crate-path decision recorded under `## Decisions to make` (likely "extend Phase 2 fork to cover news messages").
- Subscription status confirmed in `QUESTIONS.md` (which providers are active, monthly cost, additions needed).
- Coverage spot-check: historical news returns ≥10 items for AAPL over the last 24h. If <5, expand the source mix and re-run before declaring the phase done.

## Gotchas

- **TWS news pacing differs from fundamentals.** `req_historical_news` is rate-limited (specifics in IBKR docs); `req_news_article` is even tighter because each call fetches a full article body. The spike binary should sleep ~2s between requests; the production provider in Phase 7 will need an IBKR-news-specific limiter or to lean on TWS's own pacing errors as backpressure.
- **Subscription gating returns TWS error code 322 (or similar).** Distinguish "no subscription for this provider" from "no news for this symbol" from "TWS not connected" in the spike notes — these become `NewsError` variants in Phase 7.
- **`req_historical_news` requires a `provider_codes` argument.** Each provider has a string code (e.g., `BRFG` for Briefing.com, `DJ-N` for Dow Jones). The spike must capture the code list from `req_news_providers` and pass them explicitly.
- **Article body may be HTML, plain text, or a paywalled stub.** Capture one of each in the fixtures so Phase 7's parser handles the variation.
- **Coverage is not a given.** Some IBKR news feeds are heavy on macro / sector pieces and light on individual-ticker headlines. Sparse coverage for small-caps is expected and acceptable; Phase 6 must surface this so Phase 7's tests don't assume universal density.
- **Spike code is throwaway.** Don't let it become "production-ready" — Phase 7 writes the real provider from scratch with TDD.
- **The fork must compile against our existing `ibapi` patch from Phase 2.** Don't start a second fork; extend the first.
