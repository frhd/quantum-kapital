# Live quote source for the Analysis view

**Date:** 2026-04-30
**Status:** Design

## Problem

The Analysis view's "Current Price" card looks frozen. Three causes, in order of severity:

1. **`CurrentMetrics.price` is sourced from `52WeekHigh`.** `src-tauri/src/services/financial_data_service/overview.rs:55-59` reads `overview.week_52_high` into the `price` field. The UI label "Current Price" is therefore wrong — it only changes when a new 52-week high prints.
2. **Volume / change / changePercent are always blank.** `src/features/analysis/hooks/useTickerSearch.ts:72-74` sets all three to `undefined`, with a comment that Alpha Vantage `OVERVIEW` does not carry them.
3. **Fundamentals are cached for 7 days.** `cache_service` keeps the `FundamentalData` blob 7 days. Even if the AV-side fields updated, the local user wouldn't see it. This is fine for slow-moving fundamentals; it is not fine for any real-time field.

## Goals

- The Analysis view's price card shows a real, live price for the selected ticker.
- Volume, change, and change-percent are populated from the same fetch.
- Real-time fields are never cached.
- When a live quote is unavailable (TWS disconnected, no market-data permission, timeout), the user sees an inline explanation instead of stale data.

## Non-goals

- Streaming tick-level updates to the Analysis view (the polling cadence below is sufficient for a research surface).
- Alpha Vantage `GLOBAL_QUOTE` fallback when TWS is disconnected. Considered and rejected — keeps AV credits for fundamentals and matches the rest of the app, which assumes IBKR is the source of truth for live data.
- Migrating `cache_service` to SQLite or changing its TTL for fundamentals.

## Architecture

Two parallel data paths for the Analysis view:

```
Fundamentals path (existing, mostly unchanged)
  selectTicker() → ibkr_get_fundamental_data → FinancialDataService → AV OVERVIEW
                                              → cache_service (7-day TTL)

Live quote path (new)
  useQuote(symbol) → ibkr_get_quote (every 5s) → QuoteService → QuoteFetcher
                                                              → IbkrClient::get_market_data_snapshot
                                                                (real impl, new)
                                                              → MockIbkrClient (already implements)
```

`Quote` is a new, narrow value type populated from `MarketDataSnapshot`. It is never cached.

```rust
pub struct Quote {
    pub symbol: String,
    pub last_price: Option<f64>,
    pub prev_close: Option<f64>,
    pub volume: Option<i64>,
    pub timestamp: i64,
}
```

`change` and `changePercent` are computed on the frontend from `last_price - prev_close` so the backend stays trivial.

### Trait seam

A new narrow trait, mirroring the existing pattern of `HistoricalDataFetcher` (`services/historical_data_service/mod.rs:45`) and `MarketScanner` (`services/auto_scanner/mod.rs:44`):

```rust
#[async_trait]
pub trait QuoteFetcher: Send + Sync {
    async fn get_quote(&self, symbol: &str) -> Result<Quote>;
}
```

- `IbkrClient` gets an `impl QuoteFetcher` that calls a new inherent `get_market_data_snapshot` method and converts the snapshot into a `Quote`.
- `MockIbkrClient` already returns a sensible `MarketDataSnapshot`; we add a thin `impl QuoteFetcher` that reuses it.

The broader `IbkrClientTrait` (mock-only) is **not** the seam used here. `QuoteFetcher` is per-service, matching how this codebase has been growing.

## Components

### Backend

**`src-tauri/src/ibkr/client/market_data.rs`**
Add a new inherent `get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot>` method. Use `ibapi`'s market-data subscription with `snapshot=true` so the call self-terminates after the snapshot completes. Wrap in `tokio::task::spawn_blocking` (matching the existing `subscribe_market_data` pattern). 5-second timeout — if the snapshot does not complete (bad symbol, no data subscription), return `IbkrError::Timeout`.

The existing stub `subscribe_market_data` and its Tauri command (`ibkr_subscribe_market_data` in `commands/market_data.rs`) stay untouched — they are wired to live code and out of scope for this fix.

Surface IBKR error code 354 ("Requested market data is not subscribed") as a typed `IbkrError::MarketDataPermissionDenied` variant so the UI can render a clearer message.

**`src-tauri/src/services/quote_service/`** (new module)
Owns:
- The `QuoteFetcher` trait.
- A `QuoteService<F: QuoteFetcher>` struct with `fetch_quote(symbol) -> Result<Quote>` that delegates to the fetcher and converts `MarketDataSnapshot → Quote`.
- `impl QuoteFetcher for IbkrClient` (calls `get_market_data_snapshot`, maps to `Quote`).
- `impl QuoteFetcher for MockIbkrClient` (delegates to existing snapshot impl).

**`src-tauri/src/ibkr/types/`**
Add `Quote` struct (placement: a new `quote.rs` sibling to `market_data.rs`). It is *not* a renamed `MarketDataSnapshot`; it is a UI-shaped subset.

**`src-tauri/src/ibkr/commands/analysis.rs`**
New `#[tauri::command] async fn ibkr_get_quote(state, symbol) -> Result<Quote, String>`. Pulls `Arc<QuoteService<IbkrClient>>` via `State`. Returns the typed error message.

**`src-tauri/src/services/financial_data_service/overview.rs`**
Bug fix: stop aliasing `52WeekHigh` as `price`.
- `CurrentMetrics.price: Option<f64>` (currently `f64`).
- Set to `None` in `process_current_metrics`. The Analysis view reads `price` from the live-quote path; the fundamentals path no longer claims to know it.
- Update existing tests that rely on the field being populated.

**`src-tauri/src/lib.rs`**
- Construct `QuoteService::new(ibkr_client.clone())` and `app.manage(...)` it.
- Register `ibkr_get_quote` in the command handler list.

### Frontend

**`src/shared/types/index.ts`**
Add `Quote` type matching the backend shape.

**`src/shared/api/ibkr.ts`**
Add `ibkrApi.getQuote(symbol: string): Promise<Quote>` wrapper around `invoke('ibkr_get_quote', ...)`. This is the only place the command name is named.

**`src/features/analysis/hooks/useQuote.ts`** (new)
- `useQuote(symbol: string | null): { quote: Quote | null, error: QuoteError | null, loading: boolean }`.
- On mount and on symbol change: immediate fetch, then `setInterval(fetch, 5000)`.
- Cleanup on unmount and on symbol change.
- Pause polling on `document.visibilitychange` to hidden; resume on visible. Triggers an immediate fetch on resume.
- Subscribe to the existing `connection-status-changed` Tauri event (emitted by `IbkrState::update_connection_status` from `AppEvent::ConnectionStatusChanged`). On `connected: false`: stop the timer, set `error = "disconnected"`. On `connected: true`: restart the timer with an immediate fetch.
- "Last good value wins" — keep the previous `quote` while between polls; only clear it if the symbol changes or we have never had data for this symbol.

**`src/features/analysis/components/TickerCards.tsx`**
Replace the existing `Current Price` block with logic that consumes `{ quote, error }`. Four render states:
1. Quote loaded → price + change/changePercent (computed in render: `last - prev_close`) + volume.
2. Disconnected → em-dashes + small grey text "Live quote unavailable — TWS not connected".
3. No market-data permission → em-dashes + "No live data permission for this symbol".
4. First-load / between-polls with no prior value → em-dashes only (no error message); once we have a value, keep showing it through subsequent fetches.

The fundamentals card (market cap, P/E, yield, name, exchange) is unchanged. The volume card and change/changePercent are now driven by `quote`, not `ticker`.

**`src/features/analysis/hooks/useTickerSearch.ts`**
- Remove the `change`, `changePercent`, `volume` fields from the `TickerData` mapping (lines 72-74 area). They no longer come from the fundamentals path.
- `TickerData` is reduced to fundamentals-only fields. The Analysis component composes `useTickerSearch` and `useQuote` independently.

**`src/features/analysis/types/index.ts`**
Drop `price`, `change`, `changePercent`, `volume` from `TickerData` — those fields belong to the live-quote path now. `TickerData` keeps `symbol`, `name`, `exchange`, `type`, `marketCap`, `pe`, `yield`. `Quote` is its own type.

## Behavior details

### Snapshot timing
- Fetch on ticker selection.
- Re-fetch every 5 seconds while the Analysis view is mounted **and** the tab/window is visible **and** TWS is connected.
- Pause without losing the last good value when the tab is hidden.
- Stop entirely on disconnect; the inline message communicates why.

### Errors
- `IbkrError::NotConnected` → `error = "disconnected"` on the hook.
- `IbkrError::MarketDataPermissionDenied` → `error = "no_permission"`.
- `IbkrError::Timeout` and any other variant → `error = "fetch_failed"`, render em-dashes (no toast — too noisy for a 5s loop).
- Last good value is preserved through transient errors.

### IBKR pacing
- One symbol, 5s interval, single tab → ~12 calls/min, well within IBKR's 50 simultaneous market-data lines and below any pacing concern.

## Testing

### Backend (TDD: red → green → refactor)

- `quote_service` unit tests: `MarketDataSnapshot → Quote` conversion (last_price, prev_close = `close`, volume copied through; missing fields propagate as `None`). Run against the mock fetcher.
- `quote_service` error mapping: `NotConnected` propagates, `MarketDataPermissionDenied` typed-error round-trips correctly, `Timeout` maps to `fetch_failed`.
- `overview.rs` regression: assert `process_current_metrics(...).price` is `None` even when the input has a `52WeekHigh`. Update the existing `test_fetch_fundamental_data` assertion since `price > 0.0` is no longer true.
- The real `IbkrClient::get_market_data_snapshot` is exercised through `QuoteFetcher` in unit tests via the mock; live-TWS coverage is manual smoke (matching how `client::historical` is treated today).

### Frontend

- `useQuote` hook (vitest + RTL with fake timers): polls at 5s, immediate fetch on mount, pauses on `visibilitychange = hidden`, resumes with immediate fetch on visible, stops on the IBKR-disconnect event, restarts on reconnect, preserves last good value through transient errors.
- `TickerCards` render test for each of the four price-card UI states.

### TDD discipline

Every backend unit goes red → green → refactor before implementation. The `QuoteFetcher` trait is the IBKR seam — no test ever requires a live TWS connection. This matches the project rule in `CLAUDE.md`.

## Out of scope / future work

- Streaming live ticks via `StreamHandle` instead of polling. Promotable later if the polling cadence proves insufficient.
- Alpha Vantage `GLOBAL_QUOTE` fallback when TWS is disconnected.
- A "last updated 2s ago" indicator on the price card.
- Reusing the new `Quote` type elsewhere in the app (e.g., the strategies layer's `MarketContext.current_quote` already takes `&MarketDataSnapshot`; not changing that here).
