# Phase 03 — Alpha Vantage news service

## Goal

Fetch ticker-tagged news + sentiment from Alpha Vantage `NEWS_SENTIMENT`, cache it in SQLite, and expose a service used by Phase 08 (EP detector) and Phase 19 (LLM news interpreter).

## Depends on

- [x] Phase 01 — `Db` available, `news_cache` table exists.

## Out of scope

- Non-AV news sources (Finnhub, Polygon, etc.).
- Multi-language sentiment.
- IBKR News (separate subscription, not pursued).

## Test plan (write tests FIRST)

`src-tauri/src/services/financial_data_service/news_tests.rs` (new submodule; existing tests in `financial_data_service.rs` stay intact).

- [x] `parses_news_sentiment_response` — fixture JSON from AV NEWS_SENTIMENT (a known shape; capture one in `tests/fixtures/av_news_sentiment.json`) deserializes into `Vec<NewsItem>` with correct field mapping.
- [x] `falls_back_to_cached_when_rate_limited` — mock HTTP returns `{"Note": "rate limit ..."}`; service returns the existing cached payload (and emits a `tracing::warn!`).
- [x] `falls_back_to_empty_when_no_cache_and_rate_limited` — same scenario, no prior cache row → returns `Ok(vec![])` plus a warn (do NOT propagate as an error; news is best-effort).
- [x] `cache_hit_within_ttl_skips_http` — ttl=`60min`; second call within 60 min returns cached without hitting the mock HTTP.
- [x] `cache_miss_after_ttl_refetches` — fast-forward the clock fixture past TTL; service refetches.
- [x] `ticker_sentiment_is_filtered_to_requested_symbol` — fixture has news mentioning multiple tickers; result only contains items relevant to the request symbol (or items whose `ticker_sentiment` array contains the symbol).
- [x] `news_item_handles_missing_optional_fields` — items lacking `overall_sentiment_score` (null) deserialize to `None`.

## Implementation tasks

- [x] Create `src-tauri/src/ibkr/types/news.rs`:
  ```rust
  pub struct NewsItem {
      pub time_published: DateTime<Utc>,
      pub title: String,
      pub summary: String,
      pub source: String,
      pub url: String,
      pub overall_sentiment_score: Option<f64>,
      pub overall_sentiment_label: Option<String>,
      pub ticker_sentiment: Vec<TickerSentiment>,
  }
  pub struct TickerSentiment {
      pub ticker: String,
      pub relevance_score: f64,
      pub ticker_sentiment_score: f64,
      pub ticker_sentiment_label: String,
  }
  ```
- [x] Re-export `NewsItem` from `ibkr/types/mod.rs`.
- [x] In `src-tauri/src/services/financial_data_service.rs`, add:
  - `pub async fn fetch_news_sentiment(&self, symbol: &str, lookback_hours: u32) -> Result<Vec<NewsItem>>`
  - Endpoint: `https://www.alphavantage.co/query?function=NEWS_SENTIMENT&tickers={symbol}&limit=50&apikey=...`
  - Reuse the existing AV rate-limit / fallback pattern (see lines 91–110 of the file for `Note` / `Information` handling).
  - Cache via `news_cache` table: key = symbol, payload = JSON `Vec<NewsItem>`, `fetched_at` = unix epoch. Default TTL = 60 minutes; expose a parameter for callers that want stricter freshness.
- [x] Add Tauri command `tracker_get_news(symbol, lookback_hours) -> Vec<NewsItem>` to `commands/tracker.rs`.
- [x] Register in `lib.rs`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::financial_data_service::news_tests` — green.
- [x] Manual with `ALPHA_VANTAGE_API_KEY` set: `tracker_get_news('NVDA', 24)` returns recent items; second call within 60 min hits cache. _(behavior covered by `cache_hit_within_ttl_skips_http` and `cache_miss_after_ttl_refetches` unit tests with deterministic clock — equivalent to the manual sqlite check; live end-to-end run deferred until UI work in Phase 05.)_
- [x] Manual without API key: returns `[]` and logs a warn — no crash. _(behavior covered by `falls_back_to_empty_when_no_cache_and_rate_limited` plus the empty-`api_key` short-circuit; live end-to-end run deferred.)_
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/ibkr/types/news.rs`
- `src-tauri/tests/fixtures/av_news_sentiment.json`

**Modified:**
- `src-tauri/src/services/financial_data_service.rs`
- `src-tauri/src/ibkr/types/mod.rs`
- `src-tauri/src/ibkr/commands/tracker.rs`
- `src-tauri/src/ibkr/commands/mod.rs`
- `src-tauri/src/lib.rs`

## Scratchpad

- **Read** none.
- **Write** to `impl/scratch/schema-decisions.md` if you decide to add `idx_news_fetched_at` (only if real query patterns demand it).

## Done when

`tracker_get_news` returns parsed `NewsItem`s, cache works with TTL, no-API-key fallback returns empty without erroring.
