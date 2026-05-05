# Phase 3 — `get_watchlist_briefing` MCP fan-out aggregator

> Part of [Behavioral assessment via MCP](master.md). See master for invariants.

**Status:** todo

**Depends on:** — (parallel-able with Phase 2; both build directly on existing services)

**Goal:** Ship a single MCP tool `get_watchlist_briefing(symbols?, lookback_days?)` that returns one composite envelope per watchlist symbol containing `{quote, bars, news, sentiment, setups, fundamentals}`. Replaces the 12-call fan-out pattern that produced this plan's "today's setup" section. Per-symbol error envelope so partial failures (e.g. AV news upstream is down) don't kill the whole call.

**Why this matters:** the morning playbook generator (Phase 5) and any LLM client asking "what's on the watchlist looking like?" needs all six data signals per symbol. Fanning out 12+ calls per question is slow, token-expensive, and brittle. One call returns everything in ~2-3s with explicit per-field freshness markers.

## End-state for this phase

- `mcp/tools/get_watchlist_briefing.rs` registers the new tool.
- One MCP call:
  ```jsonc
  {
    "as_of": 1777963066,
    "symbols": ["AMD", "TSLA", "SYM", "RDDT"],
    "items": [
      {
        "symbol": "AMD",
        "quote": { "lastPrice": 344.21, "prevClose": 341.54, "volume": 877, "timestamp": 1777963066 },
        "bars": { "bar_size": "1d", "lookback_days": 15, "items": [/* OHLCV rows */] },
        "news": { "fetched_at_unix": 1777963100, "items": [/* top 10 */], "verdict_json": null },
        "sentiment": { "items": [/* per-source samples */] },
        "setups": { "items": [/* recent setups */] },
        "fundamentals": { /* cached row or null */ },
        "errors": []   // populated on partial failures
      },
      // ...one per symbol...
    ]
  }
  ```
- Default `lookback_days = 15` (matches the daily-bar window the playbook uses).
- Default `bars.bar_size = "1d"`.
- Default `news.max_age_secs = 3600` (cached news preferred over upstream refresh).
- Concurrent fan-out: each symbol's six fetches issued in parallel via `tokio::join!`; symbols themselves issued in parallel up to a configurable concurrency (default 4).
- Per-symbol error envelope: each item includes `errors: ["news: upstream_failed", ...]` for any field that returned an error. The successful fields are still populated.

## Files

**Create:**
- `src-tauri/src/services/watchlist_briefing/mod.rs` — composer (calls each underlying service in parallel).
- `src-tauri/src/services/watchlist_briefing/types.rs` — wire DTOs (`SymbolBriefing`, `BriefingError`, `WatchlistBriefing`).
- `src-tauri/src/services/watchlist_briefing/tests.rs` — unit tests (with fakes).
- `src-tauri/src/mcp/tools/get_watchlist_briefing.rs` — MCP tool, mirrors the existing read-tool shape.

**Modify:**
- `src-tauri/src/services/mod.rs` — add `pub mod watchlist_briefing;`.
- `src-tauri/src/mcp/tools/mod.rs` — add `pub mod get_watchlist_briefing;`.
- `src-tauri/src/mcp/handler.rs` — chain in `get_watchlist_briefing_router`.

## Reuse

- `services/quote_service` (`get_quote(symbol)`).
- `services/historical_data_service` (`get_bars(symbol, bar_size, lookback_days)`).
- `services/financial_data_service` and `news_cache.rs` (`get_news(symbol, max_age_secs)`).
- `services/social_sentiment` (`get_sentiment(symbol, since_unix?)`).
- `services/tracker_service` (`get_setups(symbol, since)`).
- `services/manual_fundamentals_store` + `services/fundamentals_provider` (`get_fundamentals(symbol)`).
- Watchlist read: `services/tracker_service::watchlist(...)` (the same backing store used by `get_watchlist`).
- The existing per-symbol MCP tool implementations in `mcp/tools/quote.rs`, `bars.rs`, `news.rs`, `get_sentiment.rs`, `setups.rs`, `fundamentals.rs` — read these to understand the call signatures and DTOs each underlying service exposes.

## End-state types

```rust
// services/watchlist_briefing/types.rs
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolBriefing {
    pub symbol: String,
    /// Each field is `Option<Value>` so missing-due-to-error and missing-due-to-no-data
    /// are distinguishable via the parallel `errors` list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bars: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub news: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setups: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fundamentals: Option<Value>,
    /// `["news: upstream_failed", "sentiment: cache miss"]` etc.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistBriefing {
    pub as_of: i64,
    pub symbols: Vec<String>,
    pub items: Vec<SymbolBriefing>,
}
```

## Tasks

### Task 1: Composer skeleton + types

**Files:**
- Create: `src-tauri/src/services/watchlist_briefing/types.rs` (paste block above)
- Create: `src-tauri/src/services/watchlist_briefing/mod.rs`

- [ ] **Step 1: Module root**

```rust
//! `watchlist_briefing` — fan-out composer that produces a single
//! `WatchlistBriefing` per call by issuing the six per-symbol read
//! services in parallel and packaging the results with a per-symbol
//! error envelope.

pub mod types;

pub use types::{SymbolBriefing, WatchlistBriefing};

#[cfg(test)]
mod tests;
```

(Defer the composer impl to Task 2; we want a failing test first.)

- [ ] **Step 2: Wire into `services/mod.rs`** (`pub mod watchlist_briefing;`).

- [ ] **Step 3: cargo check** — should compile (types module is standalone).

### Task 2: Failing test for the composer

**Files:**
- Create: `src-tauri/src/services/watchlist_briefing/tests.rs`

- [ ] **Step 1: Sketch the test using injected closures (no DB)**

The composer's signature accepts a set of trait-object fetchers so tests can inject canned responses. This avoids constructing the full service graph.

```rust
//! Composer tests — uses a `BriefingFetchers` value built from
//! closures so we can drive each underlying read in isolation.

use super::*;
use serde_json::json;

#[tokio::test]
async fn composes_one_briefing_per_symbol() {
    let fetchers = test_fetchers_ok();
    let out = compose(
        vec!["AMD".to_string(), "TSLA".to_string()],
        BriefingOpts { lookback_days: 15, bars_size: "1d".into(), news_max_age_secs: 3600, concurrency: 2 },
        &fetchers,
    )
    .await;

    assert_eq!(out.items.len(), 2);
    assert_eq!(out.items[0].symbol, "AMD");
    assert_eq!(out.items[0].errors, Vec::<String>::new());
    assert!(out.items[0].quote.is_some());
    assert!(out.items[0].bars.is_some());
    assert!(out.items[0].news.is_some());
}

#[tokio::test]
async fn partial_failure_isolates_per_field() {
    let mut f = test_fetchers_ok();
    f.fetch_news = Box::new(|_sym| Box::pin(async { Err("upstream_failed".into()) }));
    let out = compose(
        vec!["AMD".into()],
        BriefingOpts::default(),
        &f,
    )
    .await;
    assert_eq!(out.items.len(), 1);
    let it = &out.items[0];
    assert!(it.news.is_none(), "news should be missing");
    assert!(it.quote.is_some(), "quote should still be present");
    assert!(
        it.errors.iter().any(|e| e.contains("news") && e.contains("upstream_failed")),
        "errors: {:?}",
        it.errors,
    );
}

fn test_fetchers_ok() -> BriefingFetchers {
    BriefingFetchers {
        fetch_quote: Box::new(|sym| {
            let s = sym.to_string();
            Box::pin(async move { Ok(json!({"symbol": s, "lastPrice": 100.0})) })
        }),
        fetch_bars: Box::new(|_sym, _size, _lookback| {
            Box::pin(async { Ok(json!({"items": [], "count": 0})) })
        }),
        fetch_news: Box::new(|_sym| Box::pin(async { Ok(json!({"items": []})) })),
        fetch_sentiment: Box::new(|_sym| Box::pin(async { Ok(json!({"items": []})) })),
        fetch_setups: Box::new(|_sym| Box::pin(async { Ok(json!({"items": []})) })),
        fetch_fundamentals: Box::new(|_sym| Box::pin(async { Ok(json!(null)) })),
    }
}
```

- [ ] **Step 2: Run, verify it fails**

Compile error: `compose`, `BriefingFetchers`, `BriefingOpts` don't exist.

### Task 3: Implement the composer

**Files:**
- Modify: `src-tauri/src/services/watchlist_briefing/mod.rs`

- [ ] **Step 1: Implement**

```rust
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use serde_json::Value;

pub mod types;
#[cfg(test)]
mod tests;

pub use types::{SymbolBriefing, WatchlistBriefing};

type FetchResult = Result<Value, String>;
type Future01<'a> = Pin<Box<dyn Future<Output = FetchResult> + Send + 'a>>;
type SymbolFetcher = Box<dyn Fn(&str) -> Future01<'static> + Send + Sync>;
type BarsFetcher =
    Box<dyn Fn(&str, &str, u32) -> Future01<'static> + Send + Sync>;

pub struct BriefingFetchers {
    pub fetch_quote: SymbolFetcher,
    pub fetch_bars: BarsFetcher,
    pub fetch_news: SymbolFetcher,
    pub fetch_sentiment: SymbolFetcher,
    pub fetch_setups: SymbolFetcher,
    pub fetch_fundamentals: SymbolFetcher,
}

#[derive(Debug, Clone)]
pub struct BriefingOpts {
    pub lookback_days: u32,
    pub bars_size: String,
    pub news_max_age_secs: u32,
    pub concurrency: usize,
}

impl Default for BriefingOpts {
    fn default() -> Self {
        Self {
            lookback_days: 15,
            bars_size: "1d".into(),
            news_max_age_secs: 3600,
            concurrency: 4,
        }
    }
}

pub async fn compose(
    symbols: Vec<String>,
    opts: BriefingOpts,
    f: &BriefingFetchers,
) -> WatchlistBriefing {
    let f = Arc::new(f);
    let mut tasks = FuturesUnordered::new();
    for sym in symbols.iter().cloned() {
        let f = Arc::clone(&f);
        let opts = opts.clone();
        tasks.push(async move {
            let (q, b, n, sent, set, fund) = tokio::join!(
                (f.fetch_quote)(&sym),
                (f.fetch_bars)(&sym, &opts.bars_size, opts.lookback_days),
                (f.fetch_news)(&sym),
                (f.fetch_sentiment)(&sym),
                (f.fetch_setups)(&sym),
                (f.fetch_fundamentals)(&sym),
            );
            into_briefing(sym, q, b, n, sent, set, fund)
        });
    }
    let mut items: Vec<SymbolBriefing> = Vec::with_capacity(symbols.len());
    while let Some(item) = tasks.next().await {
        items.push(item);
    }
    items.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    WatchlistBriefing {
        as_of: Utc::now().timestamp(),
        symbols,
        items,
    }
}

fn into_briefing(
    symbol: String,
    quote: FetchResult,
    bars: FetchResult,
    news: FetchResult,
    sentiment: FetchResult,
    setups: FetchResult,
    fundamentals: FetchResult,
) -> SymbolBriefing {
    let mut errors = Vec::new();
    let q = field("quote", quote, &mut errors);
    let b = field("bars", bars, &mut errors);
    let n = field("news", news, &mut errors);
    let s = field("sentiment", sentiment, &mut errors);
    let st = field("setups", setups, &mut errors);
    let f = field("fundamentals", fundamentals, &mut errors);
    SymbolBriefing {
        symbol,
        quote: q,
        bars: b,
        news: n,
        sentiment: s,
        setups: st,
        fundamentals: f,
        errors,
    }
}

fn field(name: &str, r: FetchResult, errors: &mut Vec<String>) -> Option<Value> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(format!("{name}: {e}"));
            None
        }
    }
}
```

- [ ] **Step 2: Run the tests, verify they pass**

```bash
cd src-tauri && cargo test services::watchlist_briefing
```

Expected: PASS for both `composes_one_briefing_per_symbol` and `partial_failure_isolates_per_field`.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/watchlist_briefing/ src-tauri/src/services/mod.rs
git commit -m "feat(watchlist_briefing): composer with per-field error envelope"
```

### Task 4: MCP tool wrapping the composer with prod fetchers

**Files:**
- Create: `src-tauri/src/mcp/tools/get_watchlist_briefing.rs`
- Modify: `src-tauri/src/mcp/tools/mod.rs`, `src-tauri/src/mcp/handler.rs`

- [ ] **Step 1: Inventory the existing per-symbol services**

```bash
grep -nl "pub async fn get_quote\|pub fn get_quote" src-tauri/src/services/
grep -nl "pub async fn get_bars\|pub async fn fetch_bars" src-tauri/src/services/
grep -nl "pub async fn get_news\|pub async fn news_for_symbol" src-tauri/src/services/
```

For each underlying service, find the call site used by the existing per-symbol MCP tool (`mcp/tools/quote.rs`, `bars.rs`, `news.rs`, `get_sentiment.rs`, `setups.rs`, `fundamentals.rs`). Mirror those call patterns when building the prod fetchers.

- [ ] **Step 2: Implement the tool**

```rust
//! `get_watchlist_briefing` — single MCP call returning quote+bars+news+
//! sentiment+setups+fundamentals for every (or a filtered subset of)
//! watchlist symbol. Per-symbol error envelope; concurrent fan-out.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::map_tool_result;
use crate::services::watchlist_briefing::{compose, BriefingFetchers, BriefingOpts};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetWatchlistBriefingArgs {
    /// Optional symbol allow-list. Omit to brief every watchlist row.
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// Daily bars lookback. Defaults to 15.
    #[serde(default)]
    pub lookback_days: Option<u32>,
    /// Bar size; defaults to "1d".
    #[serde(default)]
    pub bar_size: Option<String>,
    /// News cache freshness window (seconds). Defaults to 3600.
    #[serde(default)]
    pub news_max_age_secs: Option<u32>,
}

#[tool_router(router = get_watchlist_briefing_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_watchlist_briefing",
        description = "One-shot per-symbol briefing for the watchlist (or a `symbols` subset). Returns `{ as_of, symbols: [...], items: [{ symbol, quote, bars, news, sentiment, setups, fundamentals, errors }, ...] }` with each constituent fetched concurrently. Per-symbol `errors[]` lists any partial failures (e.g. `\"news: upstream_failed\"`) — successful fields remain populated. Defaults: bars=15d daily, news cache age=3600s. Replaces the 12+ tool-call fan-out previously needed to brief the watchlist."
    )]
    pub async fn get_watchlist_briefing(
        &self,
        Parameters(args): Parameters<GetWatchlistBriefingArgs>,
    ) -> Result<CallToolResult, McpError> {
        // 1. Resolve symbol set: explicit arg, else read the live watchlist.
        let symbols = match args.symbols {
            Some(s) if !s.is_empty() => s,
            _ => match self.read_watchlist_symbols().await {
                Ok(v) => v,
                Err(e) => return map_tool_result::<(), String>(Err(e)),
            },
        };
        if symbols.is_empty() {
            return map_tool_result::<_, String>(Ok(serde_json::json!({
                "as_of": chrono::Utc::now().timestamp(),
                "symbols": [],
                "items": [],
            })));
        }

        // 2. Build the prod fetchers. Each closure forwards to the existing
        //    per-symbol MCP-handler method's underlying service call.
        let handler = self.clone(); // McpHandler is Arc-cheap; mirror clone pattern from peers
        let fetchers = BriefingFetchers {
            fetch_quote: prod_fetch_quote(&handler),
            fetch_bars: prod_fetch_bars(&handler),
            fetch_news: prod_fetch_news(&handler),
            fetch_sentiment: prod_fetch_sentiment(&handler),
            fetch_setups: prod_fetch_setups(&handler),
            fetch_fundamentals: prod_fetch_fundamentals(&handler),
        };
        let opts = BriefingOpts {
            lookback_days: args.lookback_days.unwrap_or(15),
            bars_size: args.bar_size.unwrap_or_else(|| "1d".into()),
            news_max_age_secs: args.news_max_age_secs.unwrap_or(3600),
            concurrency: 4,
        };
        let out = compose(symbols, opts, &fetchers).await;
        map_tool_result::<_, String>(Ok(serde_json::to_value(out).expect("serialize")))
    }
}

// Fetcher constructors. Each returns a Box<dyn Fn>...; the impls forward to
// the same underlying service call the corresponding single-symbol MCP tool
// uses. Consult `mcp/tools/quote.rs`, `bars.rs`, etc. for the exact paths.
//
// EXAMPLE shape (substitute the real service / method names):
fn prod_fetch_quote(handler: &McpHandler) -> /* SymbolFetcher */ ... { ... }
// (similar for bars, news, sentiment, setups, fundamentals)
```

> **The fetchers are the integration step.** Each one wraps an existing service call and converts the result into a `serde_json::Value` (or an error string). The exact service method names and signatures live in the per-symbol MCP tool files — read them and mirror.

- [ ] **Step 3: Add a `read_watchlist_symbols` helper to `McpHandler`** that returns just the symbols (case-normalized to upper). Mirror what `mcp/tools/watchlist.rs` does internally to read the watchlist.

- [ ] **Step 4: Wire the router** in `mcp/handler.rs` (chain `.merge(McpHandler::get_watchlist_briefing_router())`).

- [ ] **Step 5: Tool unit test (with mocked services or a small integration test)**

If the underlying services are easily fakeable via the existing `test_support` helpers, write an integration test in `tests/mcp_tool_call.rs` that calls `get_watchlist_briefing` against a seeded test DB with a 2-symbol watchlist and asserts the response shape. Otherwise rely on the composer's unit tests + a manual smoke test.

```bash
cd src-tauri && cargo test mcp::tools::get_watchlist_briefing
```

- [ ] **Step 6: Manual smoke test**

```bash
pnpm tauri dev   # in one shell
# in another, via the MCP socket from a Claude Code session:
mcp__quantum-kapital__get_watchlist_briefing()
```

Expected: one item per watchlist row with `quote`/`bars`/`news`/etc. populated; latency <5s for a 4-symbol watchlist.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/mcp/tools/get_watchlist_briefing.rs src-tauri/src/mcp/tools/mod.rs src-tauri/src/mcp/handler.rs
git commit -m "feat(mcp): get_watchlist_briefing — one-call per-symbol composite"
```

### Task 5: Read-only audit invariant

- [ ] **Step 1: Add the audit-invariant test mirroring Phase 2's `get_trade_legs_does_not_write_audit`.**

```bash
cd src-tauri && cargo test get_watchlist_briefing_does_not_write_audit
```

Expected: PASS.

- [ ] **Step 2: Commit.**

## Exit criteria

- [ ] `compose` correctly issues per-symbol fan-outs in parallel.
- [ ] Per-symbol error envelope: a single failed underlying call leaves the other fields populated and surfaces a descriptive `errors[]` entry.
- [ ] `get_watchlist_briefing` MCP tool returns the structured envelope.
- [ ] Tracer-bullet: from a Claude Code session, one `get_watchlist_briefing()` call replaces the 12-call fan-out from this plan's "today's setup" section. Latency under 5s for 4-symbol watchlist.
- [ ] Read-only audit invariant: 0 `mcp_audit` rows.
- [ ] `cargo clippy --all-targets -- -D warnings` clean.
- [ ] Update master Phase 3 row + this Status header to `done`.

## Gotchas

- **Underlying service freshness.** The composer doesn't override per-service caching policy. `get_quote` is always live (per its tool docstring); `get_news` honours `max_age_secs`; `get_sentiment` is read-only over the in-app scheduler's cadence; `get_bars` is cache-first. The briefing inherits all of these — surface freshness via the per-field timestamps inside each `Value` so consumers can reason about staleness.
- **Concurrency cap.** v1 sets `concurrency = 4`. If the watchlist grows past ~10 symbols, lower this to avoid IBKR rate-limit pressure (historical data has its own limiter; quotes don't).
- **`McpHandler::clone()` cost.** Make sure the handler is cheaply clonable (typically `Arc`-of-state inside). If it isn't, refactor to take `&Arc<Self>` in the tool body and capture clones into the closures explicitly.
- **`reqwest`-style error chains.** When mapping a service error to a string, use the chain (`{e:#}` or manual `.source()` walk) so the per-field `errors[]` entry is informative — "news: upstream_failed: connection refused" beats "news: error".
- **Empty watchlist.** Return `{items: []}` not an error. The morning sweep's first run on a fresh install hits this case.
- **Single-symbol mode.** When the user supplies `symbols: ["TSLA"]`, the tool still issues all six fetches concurrently for that one symbol. Useful for ad-hoc deep dives without writing a multi-tool fan-out.
- **`fundamentals_provider` may be `None` for some symbols.** Return `Some(Value::Null)` not `None` for "no fundamentals on file" — `None` should mean "fetch errored". This distinction matters for the playbook generator's prompt.
- **`get_setups` window.** Default to `since = today - 7 days`; expose as a future option if needed.
- **Backwards compat.** This tool ADDS a surface; nothing else changes. The existing per-symbol tools (`get_quote`, `get_bars`, etc.) stay — `get_watchlist_briefing` is a convenience composer, not a replacement.
