# IBKR news — payload shape, capture plan & crate-path decision

Phase 6 reference notes for the AV → IBKR migration. Companion to
`ibkr-fundamentals-xml.md`. Single source of truth for the news-side
crate-path decision and the structures Phase 7's parser will see.

> **Status of this file (2026-05-02):** doc-only research draft. The
> three exit fixtures (`AAPL_historical.json`, `AAPL_article_*.json`,
> `news_providers.json`) have **not** been captured yet — capture
> needs a TWS / IB Gateway session with at least one news
> subscription enabled. The Rust spike binary
> (`src-tauri/src/bin/ibkr_news_spike.rs`) is wired up and ready to
> run; it just needs the user to be at the desk with TWS up. See
> `QUESTIONS.md § P3` for the live-capture handoff.

## Crate-path decision

**Decision: use the released `ibapi = "2.11.x"` directly. No fork
needed for news.**

This is the **opposite** of Phase 2's fundamentals decision. The
published 2.11.2 crate exposes all four news APIs as public methods
on both the sync and async `Client`:

| Method | Sync source | Async source |
|---|---|---|
| `Client::news_providers()` | `client/sync.rs:1764` | `client/async.rs:1881` |
| `Client::historical_news(contract_id, providers, start, end, total)` | `client/sync.rs:1831` | `client/async.rs:1950` |
| `Client::news_article(provider_code, article_id)` | `client/sync.rs:1863` | `client/async.rs:1981` |
| `Client::news_bulletins(all_messages)` | `client/sync.rs:1786` | `client/async.rs:1909` (out of scope for v1) |

Wire-protocol presence is also confirmed:
- `OutgoingMessages::RequestNewsArticle = 84`,
  `RequestNewsProviders = 85`, `RequestHistoricalNews = 86`
  (`messages.rs:584-592`).
- `IncomingMessages::NewsArticle = 83`, `NewsProviders = 85`,
  `HistoricalNews = 86`, `HistoricalNewsEnd = 87`
  (`messages.rs:232-241`).
- Server-version gates: `REQ_NEWS_PROVIDERS = 115`,
  `REQ_NEWS_ARTICLE = 116`, `REQ_HISTORICAL_NEWS = 117` — any modern
  TWS / Gateway clears these.

The `MessageBus` is still `pub(crate)`, but unlike fundamentals we
don't need to bypass it: the public methods cover the full
request/response cycle. `historical_news` returns
`Subscription<NewsArticle>` — the `Subscription` iterator handles
the `HistoricalNews` → `HistoricalNewsEnd` framing for us.

Phase 4's fork (extending `ibapi` for `req_fundamental_data`) stays
orthogonal and unchanged. Phase 7 builds on the released crate.

### Rust types (from `ibapi::news`)

```rust
pub struct NewsProvider { pub code: String, pub name: String }
pub struct NewsArticle {
    pub time: OffsetDateTime,
    pub provider_code: String,
    pub article_id: String,
    pub headline: String,
    pub extra_data: String,
}
pub enum ArticleType { Text = 0, Binary = 1 }
pub struct NewsArticleBody {
    pub article_type: ArticleType,
    pub article_text: String,    // Base64 if Binary
}
```

These are the only structs the Phase 7 provider needs to map into
the existing `NewsItem` shape (see § "Mapping to `NewsItem`" below).

## Capture-spike plan

The spike binary at `src-tauri/src/bin/ibkr_news_spike.rs` is a real
Rust program (not a stub like Phase 2's fundamentals spike, since
the public API exists today). It is feature-gated behind the same
`ibkr-spike` feature so it never builds in CI / pre-commit.

Run from a clean checkout, with TWS / Gateway up and at least one
news subscription enabled:

```bash
cargo run --bin ibkr_news_spike --features ibkr-spike -- \
    --host 127.0.0.1 --port 7497 --client-id 998 --symbol AAPL
```

Outputs (all under `src-tauri/tests/fixtures/ibkr_news/`):

1. `news_providers.json` — output of `client.news_providers()` for
   the connected account. **The provider codes here gate everything
   else** — `historical_news` requires the codes you actually have.
2. `AAPL_historical.json` — output of
   `client.historical_news(conId(AAPL), &all_codes, now-24h, now, 50)`.
   Serialized as `Vec<NewsArticle>` with fields converted to JSON.
3. `AAPL_article_<id>.json` — output of
   `client.news_article(provider_code, article_id)` for the first
   item from the historical list. Captures `article_type` and the
   raw `article_text`.

The binary sleeps ~2s between requests to respect TWS pacing. It
hard-codes AAPL's `conId = 265598` so Phase 7's contract-resolution
work doesn't gate fixture capture (Phase 7 still has to build
contract → conId resolution properly via `contract_details`, but
the spike skips it for speed).

## Expected error codes

Phase 7 will fold these into a `NewsError` enum. From the IBKR docs
and the existing `ibapi` callbacks:

- **322** — request type / pacing violation.
  → `NewsError::Pacing`.
- **162** — historical data pacing exceeded (general family; news
  uses similar backpressure). → `NewsError::Pacing`.
- **430** — subscription missing for the requested provider.
  → `NewsError::NotSubscribed`. Important: this is the same code
  as fundamentals' subscription-missing error; per-message
  disambiguation will be needed in Phase 7.
- **504** — TWS not connected / client disconnected.
  → `NewsError::Disconnected`.
- **200** — "no security definition" or "no news available for
  contract". → `NewsError::NotAvailable`.
- TWS may return `HistoricalNewsEnd` immediately with zero items
  for a contract that has no recent headlines — this is a normal
  empty-result path, not an error.

## Mapping to `NewsItem`

`NewsItem` (today, populated by AV) lives at
`src-tauri/src/ibkr/types/news.rs`:

```rust
pub struct NewsItem {
    pub time_published: chrono::DateTime<chrono::Utc>,
    pub title: String,
    pub summary: String,
    pub source: String,
    pub url: String,
    pub overall_sentiment_score: Option<f64>,
    pub overall_sentiment_label: Option<String>,
    pub ticker_sentiment: Vec<TickerSentiment>,
}
```

The IBKR producer in Phase 7 will fill it like this:

| `NewsItem` field | IBKR source |
|---|---|
| `time_published` | `NewsArticle.time` (convert `OffsetDateTime` → `chrono::DateTime<Utc>`) |
| `title` | `NewsArticle.headline` |
| `summary` | First N chars of `NewsArticleBody.article_text` after stripping HTML/Base64 — **(needs live capture to verify)** which providers ship readable bodies vs. paywalled stubs |
| `source` | `NewsArticle.provider_code` looked up in the `news_providers` map → display name (e.g. `"DJ-N"` → `"Dow Jones"`) |
| `url` | Empty string — IBKR does not return canonical URLs. **(needs live capture to confirm; some providers may put URLs in `extra_data`)** |
| `overall_sentiment_score` | `None` — see `sentiment-loss-audit.md` |
| `overall_sentiment_label` | `None` — same |
| `ticker_sentiment` | `Vec::new()` — same |

## Lookback-window mapping

AV's call site uses `lookback_hours`. IBKR uses `start_time` +
`end_time` + `total_results: u8` (1–300). For parity with the
existing AV path:

```rust
let end = OffsetDateTime::now_utc();
let start = end - Duration::hours(lookback_hours as i64);
let total_results = 50u8;  // matches AV's `limit=50`
```

`historical_news` is per-call across all `provider_codes` passed in
one slice — no need to fan out per provider.

## Provider mix to verify

Subscriptions vary by region and account tier. The default verify-mix
(see Phase 6 plan):

- **Reuters Real-time News** — broad coverage, considered default.
- **Briefing.com (BRFG)** — US equities focus.
- **Dow Jones (DJ-N / DJNL)** — premium US.

Some providers (e.g. `DJ-N`) require explicit subscription and a
licence agreement clicked through in TWS → Account → Market Data
Subscriptions. The capture script lists everything the account has;
the user records which to add in `QUESTIONS.md` after running it.

## Coverage spot-check (Phase 6 exit gate)

After capture, we want ≥ **10 items** for AAPL over a 24h window
across the subscribed provider mix. If <5, that's a red flag:
either (a) the provider mix is too narrow, or (b) IBKR doesn't ship
ticker-tagged content for AAPL through the subscribed providers.
Phase 6 plan says expand the mix and re-run before declaring the
phase done. Sparse coverage for small-caps is expected and
acceptable, but AAPL is the canary.

## Streaming / bulletins — out of scope for v1

`req_news_bulletins` is exposed (`Client::news_bulletins(true)`)
and would be the path for streaming push. AV NEWS_SENTIMENT was
poll-only, so Phase 6 / 7 stays poll-only via `historical_news` for
parity. Streaming is a future optimisation — flagged as a deferred
item in the master plan, not in this scope.

## Open questions (tracked in `QUESTIONS.md`)

- None of the Rust → fixture mapping has been verified against
  captured payloads. We may discover (a) `extra_data` carries URL
  hints, (b) the article body is HTML for some providers and a
  paywalled stub for others, (c) `time` carries a UTC offset that
  needs converting. Phase 7 parser is written *after* fixtures land.
- Coverage volume for AAPL is unknown until capture runs.
- Provider-code → display-name mapping needs the actual
  `news_providers.json` to be canonical; we may want a small
  hard-coded fallback table for codes the user is likely to see
  even with limited subscriptions.
