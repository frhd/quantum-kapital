# Sentiment-loss audit — what disappears when AV news goes away

Phase 6 reference. Alpha Vantage's `NEWS_SENTIMENT` payload ships
per-article sentiment scoring on every item:

- `NewsItem.overall_sentiment_score: Option<f64>` (−1.0 … +1.0)
- `NewsItem.overall_sentiment_label: Option<String>` ("Bullish" / "Bearish" / …)
- `TickerSentiment.relevance_score: f64` (0.0 … 1.0)
- `TickerSentiment.ticker_sentiment_score: f64`
- `TickerSentiment.ticker_sentiment_label: String`

IBKR `req_historical_news` returns headlines + bodies with **no
sentiment fields**. Phase 6 must enumerate every consumer of these
fields and decide, per consumer: tolerate (drop), substitute (build
something), or block (subscribe somewhere else).

The baseline assumption from `master.md` is **tolerate**: the
existing `NewsInterpreter` produces a per-symbol `NewsVerdict` from
the LLM, which is what downstream consumers already key off when it
exists. Per-article scoring is treated as a fallback path.

## 1. Production consumers

| File:line | Consumer | Field(s) read | Use | Removal impact |
|---|---|---|---|---|
| `src-tauri/src/strategies/episodic_pivot/detector.rs:60-70` (`pick_sentiment`) | EP detector — picks max-relevance article and uses its score for conviction blending | `ticker_sentiment.ticker_sentiment_score`, `ticker_sentiment.relevance_score` | Per-article fallback when no `NewsVerdict` (`detector.rs:149` already prefers verdict) | **Graceful.** Verdict path is the primary; the per-article path is a tie-breaker. With AV gone, conviction is computed from verdict alone — slight loss of resolution when the LLM hasn't run yet, but no hard failure. |
| `src-tauri/src/services/news_interpreter/mod.rs:242-256` (`summarize_items`) | News interpreter — serializes items into the LLM prompt | `overall_sentiment_score`, `overall_sentiment_label` | Hint fields for Claude Haiku to bias tone classification | **Graceful.** Fields become `null`; the prompt's system message reasons primarily from headline/summary text. Net effect: minor loss of prompt context; the verdict the interpreter produces is unchanged in shape. |
| `src-tauri/src/services/thesis_generator/mod.rs:366-380` (`summarize_news`) | Thesis generator — Sonnet prompt construction | `overall_sentiment_label` only | One context field on each headline summary | **Graceful.** Becomes `null`; Sonnet system prompt doesn't pivot off it. |
| `src-tauri/src/mcp/tools/news.rs:60,105-107` | MCP `get_news` tool — surfaces cached `NewsItem[]` to remote MCP clients | All fields, serialized in JSON response | Lets agent-driven clients (Claude Code, etc.) reason about ticker-level sentiment | **Graceful.** Fields serialize to `null` / `0.0` / `""`. Clients see honest empty values; the tool's contract is not broken. |
| `src/features/tracker/types.ts:44-49,51-59` | Frontend TypeScript types | All fields (compile-time, not runtime) | Deserialization shape for Tauri command responses | **Graceful.** Types are already nullable; UI components that color-code or render scores will simply skip rendering. |

**Net production verdict: tolerate.** No consumer is structurally
dependent on per-article scoring; every site already handles
`None` / `0.0` paths or has a verdict-based primary path.

## 2. Test consumers

| File:line | Test | Fields exercised |
|---|---|---|
| `src-tauri/src/services/financial_data_service/news_tests.rs:133-143` | `parses_news_sentiment_response` | All five — locks the AV parser shape |
| `src-tauri/src/services/news_interpreter/tests.rs:61-76` | `news_with()` helper | All five — drives interpreter unit tests |
| `src-tauri/src/strategies/episodic_pivot/tests.rs:80-96` | `news_with_sentiment()` helper | All five — drives 8 EP detector tests |
| `src-tauri/src/services/thesis_generator/tests.rs:131-141` | `sample_news()` helper | `overall_sentiment_score`, `overall_sentiment_label` |
| `src-tauri/src/strategies/config_tests.rs:184-195` | Config deserialize test | `overall_sentiment_score`, `overall_sentiment_label`, `ticker_sentiment.*` — round-trip serde |
| `src-tauri/tests/fixtures/av_news_sentiment.json:1-79` | Test fixture (JSON) | Pre-populated AV response stub used by the parser tests above |

These tests stay relevant during Phases 6 / 7 — they exercise the
field reads regardless of producer. After Phase 8 (AV deletion),
the AV-specific fixture and the `parses_news_sentiment_response`
test get deleted with the AV news fetcher; the rest stay (the
fields remain on `NewsItem`, just unpopulated by the IBKR producer).

## 3. Producers (writers) — what disappears

| File:line | Writer |
|---|---|
| `src-tauri/src/services/financial_data_service/news.rs:132-144` (`parse_news_item`) | Reads `overall_sentiment_score` + `overall_sentiment_label` from raw AV JSON. Deleted in Phase 8. |
| `src-tauri/src/services/financial_data_service/news.rs:158-173` (`parse_ticker_sentiment`) | Reads `relevance_score`, `ticker_sentiment_score`, `ticker_sentiment_label`. Deleted in Phase 8. |
| `src-tauri/tests/fixtures/av_news_sentiment.json` | Raw AV response captured for parser tests. Deleted with the AV news fetcher in Phase 8. |

After Phase 8, **no production code writes these fields**. The
struct fields stay on `NewsItem` (the contract doesn't change), but
the IBKR news provider in Phase 7 will populate them with `None` /
`Vec::new()`.

## 4. Decision per consumer

All entries above resolve to **tolerate**. No consumer is being
escalated for a substitute build. If a future requirement appears
(e.g., a chart that color-codes individual headlines by score), the
substitute would be a small per-article LLM scoring pass — out of
scope for Phases 6–8 and explicitly noted in `master.md` as
acceptable trade-off.

The `NewsInterpreter` per-symbol `NewsVerdict` (from
`services/news_interpreter/`) is the primary downstream signal and
is unaffected by the producer swap — it reads the same `NewsItem`
shape regardless of source.

## 5. Sentinel test for Phase 8

When the AV news fetcher is deleted, add a regression test that
asserts a freshly fetched `NewsItem` from the IBKR provider has:

- `overall_sentiment_score == None`
- `overall_sentiment_label == None`
- `ticker_sentiment.is_empty() == true`

This locks the documented "tolerate" decision in code so a future
contributor can't quietly start populating fields from a non-LLM
source without revisiting this audit.
