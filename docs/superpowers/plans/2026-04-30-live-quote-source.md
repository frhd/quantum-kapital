# Live quote source for the Analysis view — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the static "Current Price" on the Analysis view (currently aliased to `52WeekHigh`) with a real, polled IBKR quote, and fix the underlying bug in the Alpha Vantage fundamentals path.

**Architecture:** Two parallel data paths. The existing `ibkr_get_fundamental_data` path keeps serving slow-moving fundamentals (with the price-aliasing bug removed). A new `ibkr_get_quote` path returns a live quote from `IbkrClient::get_market_data_snapshot`, polled every 5s by a `useQuote` hook that pauses on hidden tabs and stops on TWS disconnect. `Quote` is a narrow, never-cached value type sitting alongside `MarketDataSnapshot`. The IBKR seam is a new per-service `QuoteFetcher` trait following the existing `HistoricalDataFetcher` / `MarketScanner` pattern.

**Tech Stack:** Rust (Tauri 2, `ibapi 2.11`, `async-trait`, `tokio`, `thiserror`); React 19 + TypeScript (Vite, `@tauri-apps/api/event`, vitest + React Testing Library).

---

## File map

**Backend — created:**
- `src-tauri/src/ibkr/types/quote.rs` — `Quote` struct (UI-shaped, narrow).
- `src-tauri/src/services/quote_service/mod.rs` — `QuoteFetcher` trait + `QuoteService<F>`.
- `src-tauri/src/services/quote_service/tests.rs` — unit tests.

**Backend — modified:**
- `src-tauri/src/ibkr/types.rs` — declare and re-export `quote` module.
- `src-tauri/src/ibkr/error.rs` — add `IbkrError::MarketDataPermissionDenied` and `IbkrError::Timeout` variants.
- `src-tauri/src/ibkr/client/market_data.rs` — add `IbkrClient::get_market_data_snapshot` inherent method.
- `src-tauri/src/ibkr/mocks.rs` — add `impl QuoteFetcher for MockIbkrClient` and a `set_quote` setter.
- `src-tauri/src/services/mod.rs` — declare `quote_service`.
- `src-tauri/src/services/financial_data_service/overview.rs` — stop aliasing `52WeekHigh` as `price`.
- `src-tauri/src/services/financial_data_service/mod.rs` — update affected test assertions.
- `src-tauri/src/ibkr/types/fundamentals.rs` — `CurrentMetrics.price: Option<f64>`.
- `src-tauri/src/services/projection_service/mod.rs` — handle `Option<f64>` from `CurrentMetrics.price`.
- `src-tauri/src/ibkr/commands/analysis.rs` — add `ibkr_get_quote` Tauri command.
- `src-tauri/src/lib.rs` — construct `QuoteService`, manage it, register the command.

**Frontend — created:**
- `src/features/analysis/hooks/useQuote.ts` — polling hook.
- `src/features/analysis/hooks/useQuote.test.ts` — vitest unit tests.

**Frontend — modified:**
- `src/shared/types/analysis.ts` — `CurrentMetrics.price?: number`; new `Quote` type.
- `src/shared/api/ibkr.ts` — `getQuote(symbol)` wrapper.
- `src/features/analysis/types/index.ts` — drop `price`, `change`, `changePercent`, `volume` from `TickerData`.
- `src/features/analysis/hooks/useTickerSearch.ts` — drop the same fields from the mapping.
- `src/features/analysis/components/TickerCards.tsx` — accept a `quote` prop, render four states.
- `src/features/analysis/components/TickerAnalysis.tsx` — call `useQuote`, pass `quote` into `TickerCards`.

---

## Task 1: Add `IbkrError::MarketDataPermissionDenied` and `IbkrError::Timeout` variants

**Why first:** the snapshot impl, the service, and tests all match on these variants; defining them up front lets every later task compile against a stable error type.

**Files:**
- Modify: `src-tauri/src/ibkr/error.rs`

- [ ] **Step 1: Add the variants**

Edit `src-tauri/src/ibkr/error.rs`:

```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum IbkrError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Market data not subscribed for symbol")]
    MarketDataPermissionDenied,

    #[error("Request timed out after {0}ms")]
    Timeout(u64),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<ibapi::Error> for IbkrError {
    fn from(err: ibapi::Error) -> Self {
        IbkrError::ApiError(err.to_string())
    }
}

impl From<serde_json::Error> for IbkrError {
    fn from(err: serde_json::Error) -> Self {
        IbkrError::SerializationError(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, IbkrError>;
```

- [ ] **Step 2: Confirm compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS (no new warnings).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/ibkr/error.rs
git commit -m "feat(ibkr): add MarketDataPermissionDenied and Timeout error variants"
```

---

## Task 2: Add `Quote` type

**Files:**
- Create: `src-tauri/src/ibkr/types/quote.rs`
- Modify: `src-tauri/src/ibkr/types.rs`

- [ ] **Step 1: Create the `Quote` struct**

Create `src-tauri/src/ibkr/types/quote.rs`:

```rust
use serde::{Deserialize, Serialize};

/// A live, never-cached, UI-shaped quote. Sourced from
/// `MarketDataSnapshot` via `QuoteService`. Distinct from
/// `MarketDataSnapshot` because the UI only needs four fields and
/// because future quote sources need not match the snapshot shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub symbol: String,
    /// Last traded price (regular or delayed, depending on TWS data
    /// permissions). `None` if no last tick was received before the
    /// snapshot end.
    pub last_price: Option<f64>,
    /// Previous session's close. Used by the frontend to compute
    /// change and change-percent.
    pub prev_close: Option<f64>,
    /// Cumulative session volume.
    pub volume: Option<i32>,
    /// Unix epoch seconds when the snapshot completed.
    pub timestamp: i64,
}
```

- [ ] **Step 2: Wire the module into `ibkr/types.rs`**

Edit `src-tauri/src/ibkr/types.rs`:

```rust
// This file now re-exports all types from the sub-modules
// for backward compatibility and convenience

pub mod account;
pub mod connection;
pub mod fundamentals;
pub mod market_data;
pub mod orders;
pub mod positions;
pub mod quote;

pub mod historical;
pub mod news;
pub mod scanner;
pub mod tracker;

// Re-export all types at the root level for backward compatibility
pub use account::*;
pub use connection::*;
pub use fundamentals::*;
pub use market_data::*;
pub use orders::*;
pub use positions::*;
pub use quote::*;

#[allow(unused_imports)]
pub use historical::*;
#[allow(unused_imports)]
pub use news::*;
pub use scanner::*;
#[allow(unused_imports)]
pub use tracker::*;
```

- [ ] **Step 3: Confirm compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/ibkr/types/quote.rs src-tauri/src/ibkr/types.rs
git commit -m "feat(ibkr): add Quote type for live market quotes"
```

---

## Task 3: Define `QuoteFetcher` trait and `QuoteService` (test-first)

**Files:**
- Create: `src-tauri/src/services/quote_service/mod.rs`
- Modify: `src-tauri/src/services/mod.rs`

- [ ] **Step 1: Declare the new module**

Edit `src-tauri/src/services/mod.rs`. Add `pub mod quote_service;` alongside the other module declarations (placement: alphabetical, between `projection_service` and `thesis_generator` if those are the neighbors; otherwise wherever makes sense for the file's existing ordering).

- [ ] **Step 2: Write the failing tests first**

Create `src-tauri/src/services/quote_service/mod.rs`:

```rust
use std::sync::Arc;

use async_trait::async_trait;

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::{MarketDataSnapshot, Quote};

/// Narrow IBKR seam for the live-quote path. Mirrors the
/// `HistoricalDataFetcher` / `MarketScanner` pattern: the real client
/// implements this trait inherently, the mock implements it via
/// `MockIbkrClient`, and tests only ever depend on this trait.
#[async_trait]
pub trait QuoteFetcher: Send + Sync {
    async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot>;
}

pub struct QuoteService {
    fetcher: Arc<dyn QuoteFetcher>,
}

impl QuoteService {
    pub fn new(fetcher: Arc<dyn QuoteFetcher>) -> Self {
        Self { fetcher }
    }

    /// Fetches a `MarketDataSnapshot` and projects it into a `Quote`.
    /// Errors propagate untranslated — the Tauri command layer maps
    /// them to user-facing strings.
    pub async fn fetch_quote(&self, symbol: &str) -> Result<Quote> {
        let snapshot = self.fetcher.get_market_data_snapshot(symbol).await?;
        Ok(snapshot_to_quote(snapshot))
    }
}

fn snapshot_to_quote(snapshot: MarketDataSnapshot) -> Quote {
    Quote {
        symbol: snapshot.symbol,
        last_price: snapshot.last_price,
        prev_close: snapshot.close,
        volume: snapshot.volume,
        timestamp: snapshot.timestamp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    /// Programmable mock for `QuoteFetcher`. Tests inject either a
    /// canned snapshot or a canned error.
    struct StubFetcher {
        result: Mutex<Option<Result<MarketDataSnapshot>>>,
    }

    impl StubFetcher {
        fn ok(snapshot: MarketDataSnapshot) -> Arc<Self> {
            Arc::new(Self {
                result: Mutex::new(Some(Ok(snapshot))),
            })
        }

        fn err(error: IbkrError) -> Arc<Self> {
            Arc::new(Self {
                result: Mutex::new(Some(Err(error))),
            })
        }
    }

    #[async_trait]
    impl QuoteFetcher for StubFetcher {
        async fn get_market_data_snapshot(&self, _symbol: &str) -> Result<MarketDataSnapshot> {
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("StubFetcher called more than once")
        }
    }

    fn sample_snapshot() -> MarketDataSnapshot {
        MarketDataSnapshot {
            symbol: "AAPL".to_string(),
            bid_price: Some(150.20),
            bid_size: Some(100),
            ask_price: Some(150.30),
            ask_size: Some(200),
            last_price: Some(150.25),
            last_size: Some(50),
            high: Some(151.00),
            low: Some(149.50),
            volume: Some(1_234_567),
            close: Some(149.80),
            open: Some(150.00),
            timestamp: 1_730_000_000,
        }
    }

    #[tokio::test]
    async fn fetch_quote_maps_snapshot_fields() {
        let fetcher = StubFetcher::ok(sample_snapshot());
        let service = QuoteService::new(fetcher);

        let quote = service.fetch_quote("AAPL").await.expect("ok");

        assert_eq!(quote.symbol, "AAPL");
        assert_eq!(quote.last_price, Some(150.25));
        assert_eq!(quote.prev_close, Some(149.80));
        assert_eq!(quote.volume, Some(1_234_567));
        assert_eq!(quote.timestamp, 1_730_000_000);
    }

    #[tokio::test]
    async fn fetch_quote_propagates_missing_fields_as_none() {
        let mut snapshot = sample_snapshot();
        snapshot.last_price = None;
        snapshot.close = None;
        snapshot.volume = None;

        let fetcher = StubFetcher::ok(snapshot);
        let service = QuoteService::new(fetcher);

        let quote = service.fetch_quote("AAPL").await.expect("ok");

        assert_eq!(quote.last_price, None);
        assert_eq!(quote.prev_close, None);
        assert_eq!(quote.volume, None);
    }

    #[tokio::test]
    async fn fetch_quote_propagates_not_connected() {
        let fetcher = StubFetcher::err(IbkrError::NotConnected);
        let service = QuoteService::new(fetcher);

        let err = service.fetch_quote("AAPL").await.expect_err("err");
        assert!(matches!(err, IbkrError::NotConnected));
    }

    #[tokio::test]
    async fn fetch_quote_propagates_market_data_permission_denied() {
        let fetcher = StubFetcher::err(IbkrError::MarketDataPermissionDenied);
        let service = QuoteService::new(fetcher);

        let err = service.fetch_quote("AAPL").await.expect_err("err");
        assert!(matches!(err, IbkrError::MarketDataPermissionDenied));
    }

    #[tokio::test]
    async fn fetch_quote_propagates_timeout() {
        let fetcher = StubFetcher::err(IbkrError::Timeout(5_000));
        let service = QuoteService::new(fetcher);

        let err = service.fetch_quote("AAPL").await.expect_err("err");
        assert!(matches!(err, IbkrError::Timeout(5_000)));
    }
}
```

- [ ] **Step 3: Run tests, confirm they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -- services::quote_service`
Expected: 5 tests pass. (The implementation in Step 2 is already minimal; this is a single red→green→refactor cycle since the production code and tests live in the same file.)

- [ ] **Step 4: Confirm clippy and fmt**

Run:
```
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/mod.rs src-tauri/src/services/quote_service/mod.rs
git commit -m "feat(quote_service): add QuoteFetcher trait and snapshot→Quote service"
```

---

## Task 4: Implement `QuoteFetcher` for `MockIbkrClient`

The mock already returns a sensible `MarketDataSnapshot` from its inherent `get_market_data_snapshot`. Here we add the trait impl so `MockIbkrClient` can be plugged into `QuoteService` in tests.

**Files:**
- Modify: `src-tauri/src/ibkr/mocks.rs`

- [ ] **Step 1: Add the trait impl**

Append to `src-tauri/src/ibkr/mocks.rs` (anywhere after the existing `impl IbkrClientTrait for MockIbkrClient` block; group with other trait impls):

```rust
#[async_trait]
impl crate::services::quote_service::QuoteFetcher for MockIbkrClient {
    async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot> {
        // Delegate to the inherent trait impl already on this type.
        <Self as IbkrClientTrait>::get_market_data_snapshot(self, symbol).await
    }
}
```

- [ ] **Step 2: Add a regression test using the mock through the service**

Add to the `tests` module in `src-tauri/src/services/quote_service/mod.rs`:

```rust
    #[tokio::test]
    async fn quote_service_works_with_mock_ibkr_client() {
        use crate::ibkr::mocks::MockIbkrClient;

        let mock = Arc::new(MockIbkrClient::new());
        mock.set_connected(true).await;

        let service = QuoteService::new(mock as Arc<dyn QuoteFetcher>);
        let quote = service.fetch_quote("AAPL").await.expect("ok");

        // Mock canned values from mocks.rs
        assert_eq!(quote.last_price, Some(150.35));
        assert_eq!(quote.prev_close, Some(149.80));
        assert_eq!(quote.volume, Some(1_234_567));
    }

    #[tokio::test]
    async fn quote_service_fails_when_mock_disconnected() {
        use crate::ibkr::mocks::MockIbkrClient;

        let mock = Arc::new(MockIbkrClient::new());
        // do not connect

        let service = QuoteService::new(mock as Arc<dyn QuoteFetcher>);
        let err = service.fetch_quote("AAPL").await.expect_err("err");
        assert!(matches!(err, IbkrError::NotConnected));
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -- services::quote_service`
Expected: 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/ibkr/mocks.rs src-tauri/src/services/quote_service/mod.rs
git commit -m "feat(quote_service): wire MockIbkrClient through QuoteFetcher"
```

---

## Task 5: Implement `IbkrClient::get_market_data_snapshot`

This is the only step that talks to live TWS. Unit tests cannot exercise it (matching how `client::historical` is treated today); coverage is the trait-level tests in Task 3 + manual smoke at the end.

**Files:**
- Modify: `src-tauri/src/ibkr/client/market_data.rs`

- [ ] **Step 1: Replace the file with the new method (keep existing `subscribe_market_data`)**

Edit `src-tauri/src/ibkr/client/market_data.rs`:

```rust
use std::time::Duration;

use ibapi::contracts::Contract;
use ibapi::contracts::tick_types::TickType;
use ibapi::market_data::realtime::TickTypes;

use crate::ibkr::error::{IbkrError, Result};
use crate::ibkr::types::MarketDataSnapshot;

use super::IbkrClient;

const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

impl IbkrClient {
    /// Existing best-effort subscription. Kept because
    /// `ibkr_subscribe_market_data` Tauri command depends on it.
    pub async fn subscribe_market_data(&self, symbol: &str) -> Result<()> {
        let client_clone = self.ibapi_client().await?;

        let symbol = symbol.to_string();

        tokio::task::spawn_blocking(move || {
            let contract = Contract::stock(&symbol).build();
            let tick_types = &["233"]; // RTVolume
            match client_clone
                .market_data(&contract)
                .generic_ticks(tick_types)
                .subscribe()
            {
                Ok(_subscription) => Ok(()),
                Err(e) => Err(IbkrError::from(e)),
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }

    /// One-shot snapshot of level-1 market data for `symbol`.
    ///
    /// Uses ibapi's `snapshot=true` mode so the server pushes a fixed
    /// burst of ticks then sends `SnapshotEnd` — we drain those ticks
    /// and return as soon as `SnapshotEnd` arrives or `SNAPSHOT_TIMEOUT`
    /// elapses (whichever comes first).
    ///
    /// Errors:
    /// - `IbkrError::NotConnected` if there is no live ibapi client.
    /// - `IbkrError::MarketDataPermissionDenied` if TWS replies with
    ///   error code 354 ("Requested market data is not subscribed").
    /// - `IbkrError::Timeout` if no `SnapshotEnd` arrives within
    ///   `SNAPSHOT_TIMEOUT`.
    /// - `IbkrError::ApiError` for any other ibapi error.
    pub async fn get_market_data_snapshot(
        &self,
        symbol: &str,
    ) -> Result<MarketDataSnapshot> {
        let client_clone = self.ibapi_client().await?;
        let symbol_owned = symbol.to_string();

        tokio::task::spawn_blocking(move || -> Result<MarketDataSnapshot> {
            let contract = Contract::stock(&symbol_owned).build();
            let generic_ticks: Vec<&str> = Vec::new();

            let subscription = client_clone
                .market_data(&contract)
                .generic_ticks(&generic_ticks)
                .snapshot()
                .subscribe()
                .map_err(IbkrError::from)?;

            let mut snapshot = MarketDataSnapshot {
                symbol: symbol_owned.clone(),
                bid_price: None,
                bid_size: None,
                ask_price: None,
                ask_size: None,
                last_price: None,
                last_size: None,
                high: None,
                low: None,
                volume: None,
                close: None,
                open: None,
                timestamp: chrono::Utc::now().timestamp(),
            };

            let deadline = std::time::Instant::now() + SNAPSHOT_TIMEOUT;

            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(IbkrError::Timeout(
                        SNAPSHOT_TIMEOUT.as_millis() as u64,
                    ));
                }

                match subscription.next_timeout(remaining) {
                    Some(TickTypes::Price(tick)) => {
                        apply_price(&mut snapshot, &tick);
                    }
                    Some(TickTypes::Size(tick)) => {
                        apply_size(&mut snapshot, &tick);
                    }
                    Some(TickTypes::PriceSize(tick)) => {
                        apply_price_size(&mut snapshot, &tick);
                    }
                    Some(TickTypes::SnapshotEnd) => {
                        snapshot.timestamp = chrono::Utc::now().timestamp();
                        return Ok(snapshot);
                    }
                    Some(TickTypes::Notice(notice)) => {
                        // ibapi delivers TWS error codes through Notice.
                        // 354 = "Requested market data is not subscribed".
                        if notice.code == 354 {
                            return Err(IbkrError::MarketDataPermissionDenied);
                        }
                        // Other notices (e.g. farm connection messages)
                        // are informational; keep looping.
                    }
                    Some(_) => {
                        // Other tick types (Generic, String, EFP,
                        // RequestParameters, etc.) aren't projected
                        // into MarketDataSnapshot — ignore.
                    }
                    None => {
                        return Err(IbkrError::Timeout(
                            SNAPSHOT_TIMEOUT.as_millis() as u64,
                        ));
                    }
                }
            }
        })
        .await
        .map_err(|e| IbkrError::Unknown(e.to_string()))?
    }
}

fn apply_price(snapshot: &mut MarketDataSnapshot, tick: &ibapi::market_data::realtime::TickPrice) {
    match tick.tick_type {
        TickType::Bid | TickType::DelayedBid => snapshot.bid_price = Some(tick.price),
        TickType::Ask | TickType::DelayedAsk => snapshot.ask_price = Some(tick.price),
        TickType::Last | TickType::DelayedLast => snapshot.last_price = Some(tick.price),
        TickType::High | TickType::DelayedHigh => snapshot.high = Some(tick.price),
        TickType::Low | TickType::DelayedLow => snapshot.low = Some(tick.price),
        TickType::Close | TickType::DelayedClose => snapshot.close = Some(tick.price),
        TickType::Open | TickType::DelayedOpen => snapshot.open = Some(tick.price),
        _ => {}
    }
}

fn apply_size(snapshot: &mut MarketDataSnapshot, tick: &ibapi::market_data::realtime::TickSize) {
    match tick.tick_type {
        TickType::BidSize | TickType::DelayedBidSize => {
            snapshot.bid_size = Some(tick.size as i32);
        }
        TickType::AskSize | TickType::DelayedAskSize => {
            snapshot.ask_size = Some(tick.size as i32);
        }
        TickType::LastSize | TickType::DelayedLastSize => {
            snapshot.last_size = Some(tick.size as i32);
        }
        TickType::Volume | TickType::DelayedVolume => {
            snapshot.volume = Some(tick.size as i32);
        }
        _ => {}
    }
}

fn apply_price_size(
    snapshot: &mut MarketDataSnapshot,
    tick: &ibapi::market_data::realtime::TickPriceSize,
) {
    match tick.price_tick_type {
        TickType::Bid | TickType::DelayedBid => snapshot.bid_price = Some(tick.price),
        TickType::Ask | TickType::DelayedAsk => snapshot.ask_price = Some(tick.price),
        TickType::Last | TickType::DelayedLast => snapshot.last_price = Some(tick.price),
        _ => {}
    }
    match tick.size_tick_type {
        TickType::BidSize | TickType::DelayedBidSize => snapshot.bid_size = Some(tick.size as i32),
        TickType::AskSize | TickType::DelayedAskSize => snapshot.ask_size = Some(tick.size as i32),
        TickType::LastSize | TickType::DelayedLastSize => {
            snapshot.last_size = Some(tick.size as i32);
        }
        _ => {}
    }
}
```

> **Note on tick types:** if any of `TickType::DelayedBid`, `DelayedAsk`, `DelayedBidSize`, `DelayedAskSize` do not exist in this ibapi version, drop the unmatched arms (the existing tests will still pass — they don't depend on bid/ask). The `TickPriceSize.price_tick_type` / `size_tick_type` field names follow the struct definition in `ibapi-2.11.2/src/market_data/realtime/mod.rs:435`.

- [ ] **Step 2: Confirm compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS. If a `TickType::Delayed*` variant is missing, remove that arm and re-run.

- [ ] **Step 3: Confirm clippy and fmt**

Run:
```
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```
Expected: PASS.

- [ ] **Step 4: Run all backend tests (regression check)**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS. (No new tests here — Task 5 has no unit-test surface.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ibkr/client/market_data.rs
git commit -m "feat(ibkr): real get_market_data_snapshot via ibapi snapshot mode"
```

---

## Task 6: Implement `QuoteFetcher` for `IbkrClient`

**Files:**
- Modify: `src-tauri/src/services/quote_service/mod.rs`

- [ ] **Step 1: Add the impl**

Append to `src-tauri/src/services/quote_service/mod.rs` (before the `#[cfg(test)] mod tests` block):

```rust
#[async_trait]
impl QuoteFetcher for crate::ibkr::client::IbkrClient {
    async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot> {
        crate::ibkr::client::IbkrClient::get_market_data_snapshot(self, symbol).await
    }
}
```

- [ ] **Step 2: Confirm compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

- [ ] **Step 3: Run service tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -- services::quote_service`
Expected: 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/quote_service/mod.rs
git commit -m "feat(quote_service): impl QuoteFetcher for IbkrClient"
```

---

## Task 7: Add `ibkr_get_quote` Tauri command

**Files:**
- Modify: `src-tauri/src/ibkr/commands/analysis.rs`

- [ ] **Step 1: Add the command**

Append to `src-tauri/src/ibkr/commands/analysis.rs`:

```rust
use std::sync::Arc;

use crate::ibkr::types::Quote;
use crate::services::quote_service::QuoteService;

/// Fetches a one-shot live quote from IBKR. Maps typed errors to
/// stable string discriminants the frontend can switch on:
///   - `"disconnected"`         → IbkrError::NotConnected
///   - `"no_permission"`        → IbkrError::MarketDataPermissionDenied
///   - `"timeout"`              → IbkrError::Timeout(..)
///   - any other variant        → its `Display` form (treated as
///                                `fetch_failed` by the UI).
#[tauri::command]
pub async fn ibkr_get_quote(
    quote_service: tauri::State<'_, Arc<QuoteService>>,
    symbol: String,
) -> Result<Quote, String> {
    use crate::ibkr::error::IbkrError;

    match quote_service.fetch_quote(&symbol).await {
        Ok(quote) => Ok(quote),
        Err(IbkrError::NotConnected) => Err("disconnected".to_string()),
        Err(IbkrError::MarketDataPermissionDenied) => Err("no_permission".to_string()),
        Err(IbkrError::Timeout(_)) => Err("timeout".to_string()),
        Err(other) => Err(other.to_string()),
    }
}
```

- [ ] **Step 2: Confirm compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS (the State binding will be wired up in Task 9; for now the function compiles standalone).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/ibkr/commands/analysis.rs
git commit -m "feat(commands): add ibkr_get_quote Tauri command"
```

---

## Task 8: Fix `overview.rs` — stop aliasing `52WeekHigh` as `price`

This is the underlying bug. Switching `CurrentMetrics.price` to `Option<f64>` is a breaking change to the type, so callers and TS bindings need to be updated in this same task.

**Files:**
- Modify: `src-tauri/src/ibkr/types/fundamentals.rs`
- Modify: `src-tauri/src/services/financial_data_service/overview.rs`
- Modify: `src-tauri/src/services/financial_data_service/mod.rs`
- Modify: `src-tauri/src/services/projection_service/mod.rs`
- Modify: `src/shared/types/analysis.ts`

- [ ] **Step 1: Update the Rust type**

Edit `src-tauri/src/ibkr/types/fundamentals.rs:108-120`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentMetrics {
    /// Optional because Alpha Vantage's OVERVIEW endpoint does not
    /// return a current price. Live price comes from the separate
    /// `Quote` path. Kept on the type so existing callers (e.g.
    /// projections, mock fixtures) can pass an explicit value when
    /// they have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    pub pe_ratio: f64,
    pub shares_outstanding: f64, // in millions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_cap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dividend_yield: Option<f64>,
}
```

- [ ] **Step 2: Stop aliasing `52WeekHigh` as `price`**

Edit `src-tauri/src/services/financial_data_service/overview.rs:41-83`:

```rust
pub(super) fn process_current_metrics(overview: &AlphaVantageOverview) -> CurrentMetrics {
    let pe_ratio = overview
        .pe_ratio
        .as_ref()
        .and_then(|pe| pe.parse::<f64>().ok())
        .unwrap_or(0.0);

    let shares_outstanding = overview
        .shares_outstanding
        .as_ref()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|s| s / 1_000_000.0)
        .unwrap_or(0.0);

    let dividend_yield = overview
        .dividend_yield
        .as_ref()
        .and_then(|dy| dy.parse::<f64>().ok());

    let market_cap = overview.market_capitalization.as_ref().map(|mc_str| {
        if let Ok(mc) = mc_str.parse::<f64>() {
            format_market_cap(mc)
        } else {
            mc_str.clone()
        }
    });

    CurrentMetrics {
        // Alpha Vantage OVERVIEW does not carry a real current price.
        // The live-quote path (ibkr_get_quote) owns this concern.
        price: None,
        pe_ratio,
        shares_outstanding,
        name: overview.name.clone(),
        exchange: overview.exchange.clone(),
        market_cap,
        dividend_yield,
    }
}
```

- [ ] **Step 3: Add a regression test asserting price is `None`**

Append to the `tests` module in `src-tauri/src/services/financial_data_service/overview.rs` (create the module if it doesn't exist; place the `#[cfg(test)] mod tests` block at the end of the file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_current_metrics_does_not_alias_52week_high_as_price() {
        let overview = AlphaVantageOverview {
            symbol: Some("AAPL".into()),
            name: Some("Apple".into()),
            exchange: Some("NASDAQ".into()),
            market_capitalization: Some("3000000000000".into()),
            pe_ratio: Some("30.0".into()),
            shares_outstanding: Some("15000000000".into()),
            week_52_high: Some("202.49".into()),
            dividend_yield: Some("0.005".into()),
        };

        let metrics = process_current_metrics(&overview);

        assert!(metrics.price.is_none(), "OVERVIEW must not populate price");
        assert!((metrics.pe_ratio - 30.0).abs() < 1e-9);
        assert_eq!(metrics.market_cap.as_deref(), Some("3.0T"));
    }
}
```

- [ ] **Step 4: Update existing fundamental-data test in `mod.rs`**

Edit `src-tauri/src/services/financial_data_service/mod.rs` around line 247 (`assert!(data.current_metrics.pe_ratio > 0.0);`). The surrounding assertion may also reference `price` — change `data.current_metrics.price > 0.0` (if present) to `data.current_metrics.price.is_none()`. Read the file first to confirm what exists:

```bash
sed -n '230,260p' src-tauri/src/services/financial_data_service/mod.rs
```

Then update any `assert!(... .price ...)` line to:

```rust
assert!(data.current_metrics.price.is_none());
```

If the test depends on a real network call (`#[ignore]` or gated), leave gating intact — only change the assertion.

- [ ] **Step 5: Update `projection_service` to handle `Option<f64>`**

Edit `src-tauri/src/services/projection_service/mod.rs` around line 57:

```rust
        let baseline = Self::create_baseline_projection(
            baseline_data,
            fundamental.current_metrics.shares_outstanding,
            fundamental.current_metrics.price.unwrap_or(0.0),
        );
```

(Justification: `create_baseline_projection` takes the current price as informational seed for the baseline year. Falling back to `0.0` when no live price is available preserves all other projection math; the live-quote path is what shows the user the real number.)

Search for any other reads of `current_metrics.price`:

```bash
grep -rn "current_metrics\.price" src-tauri/src
```

Each remaining hit should also receive `.unwrap_or(0.0)` (or appropriate handling — read the surrounding code).

Also search the `generate_mock_fundamental_data` function in `projection_service` (it's the fallback when AV is unavailable) and update the `CurrentMetrics { price: ..., .. }` literal so `price` becomes either `Some(some_default)` or `None`:

```bash
grep -n "generate_mock_fundamental_data" src-tauri/src/services/projection_service/mod.rs
```

For mocks, set `price: None` (consistent with the OVERVIEW fix — the price field belongs to the quote path).

- [ ] **Step 6: Update TS type**

Edit `src/shared/types/analysis.ts:82-90`:

```ts
export interface CurrentMetrics {
  price?: number
  peRatio: number
  sharesOutstanding: number
  name?: string
  exchange?: string
  marketCap?: string
  dividendYield?: number
}
```

- [ ] **Step 7: Confirm everything compiles and tests pass**

Run:
```
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm typecheck
```
Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/ibkr/types/fundamentals.rs \
        src-tauri/src/services/financial_data_service/overview.rs \
        src-tauri/src/services/financial_data_service/mod.rs \
        src-tauri/src/services/projection_service/mod.rs \
        src/shared/types/analysis.ts
git commit -m "fix(overview): stop aliasing 52WeekHigh as price"
```

---

## Task 9: Wire `QuoteService` and the new command into `lib.rs`

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Construct and manage the service**

In `src-tauri/src/lib.rs`, after the `hist_service` block (around line 92, where `Arc<dyn HistoricalDataFetcher>` is set up) add:

```rust
            // Phase 21: live-quote service. Wraps IbkrClient through
            // the QuoteFetcher seam so the command + tests share the
            // same interface, and the snapshot is never cached.
            let quote_fetcher: Arc<dyn crate::services::quote_service::QuoteFetcher> =
                Arc::clone(&ibkr_state.client)
                    as Arc<dyn crate::services::quote_service::QuoteFetcher>;
            let quote_service =
                Arc::new(crate::services::quote_service::QuoteService::new(quote_fetcher));
```

After the existing `app.manage(...)` calls (around line 218):

```rust
            app.manage(quote_service);
```

- [ ] **Step 2: Register the Tauri command**

In the `tauri::generate_handler![...]` list (around line 221+), add `ibkr::commands::ibkr_get_quote,`. Match the surrounding indentation; the order should sit alongside other analysis commands (e.g. next to `ibkr_get_fundamental_data`).

If the file uses a separate `pub use` re-export at the top of `commands/mod.rs`, add `ibkr_get_quote` there too. Verify with:

```bash
grep -n "ibkr_get_fundamental_data\|ibkr_get_quote" src-tauri/src/ibkr/commands/mod.rs
```

- [ ] **Step 3: Compile, lint, test**

Run:
```
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/ibkr/commands/mod.rs
git commit -m "feat(lib): manage QuoteService and register ibkr_get_quote"
```

---

## Task 10: Add `Quote` TS type and API wrapper

**Files:**
- Modify: `src/shared/types/analysis.ts`
- Modify: `src/shared/api/ibkr.ts`

- [ ] **Step 1: Add the type**

Append to `src/shared/types/analysis.ts`:

```ts
export interface Quote {
  symbol: string
  lastPrice?: number
  prevClose?: number
  volume?: number
  /** Unix epoch seconds when the snapshot completed. */
  timestamp: number
}
```

- [ ] **Step 2: Add the API wrapper**

Edit `src/shared/api/ibkr.ts`. Add `Quote` to the type imports at the top:

```ts
import type {
  // ...existing imports...
  Quote,
} from "../types"
```

In the `ibkrApi` object, after `getFundamentalData`, add:

```ts
  getQuote: async (symbol: string) => {
    return invoke<Quote>("ibkr_get_quote", { symbol })
  },
```

- [ ] **Step 3: Confirm typecheck**

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/shared/types/analysis.ts src/shared/api/ibkr.ts
git commit -m "feat(api): add Quote type and getQuote wrapper"
```

---

## Task 11: Trim `TickerData` and `useTickerSearch`

The fundamentals path no longer claims to know price/volume/change — those move to the live-quote path.

**Files:**
- Modify: `src/features/analysis/types/index.ts`
- Modify: `src/features/analysis/hooks/useTickerSearch.ts`

- [ ] **Step 1: Trim the type**

Replace `src/features/analysis/types/index.ts` with:

```ts
export interface TickerSearchResult {
  symbol: string
  name: string
  exchange: string
  type: string
}

export interface TickerData {
  symbol: string
  name: string
  exchange: string
  type: string
  marketCap?: string
  pe?: number
  yield?: number
}
```

- [ ] **Step 2: Drop the dropped fields from the mapping**

Edit `src/features/analysis/hooks/useTickerSearch.ts:64-78`:

```ts
      const tickerData: TickerData = {
        symbol: fundamentalData.symbol,
        name: fundamentalData.currentMetrics.name || normalizedSymbol,
        exchange: fundamentalData.currentMetrics.exchange || "Unknown",
        type: "Stock",
        marketCap: fundamentalData.currentMetrics.marketCap || undefined,
        pe: fundamentalData.currentMetrics.peRatio,
        yield: fundamentalData.currentMetrics.dividendYield,
      }
```

(The `// Note: Alpha Vantage OVERVIEW doesn't provide change/changePercent...` comment can be deleted — the explanation now lives on the live-quote path.)

- [ ] **Step 3: Confirm typecheck and lint**

Run:
```
pnpm typecheck
pnpm lint
```
Expected: PASS. If `pnpm lint` flags any unused imports in `useTickerSearch.ts`, remove them.

- [ ] **Step 4: Commit**

```bash
git add src/features/analysis/types/index.ts src/features/analysis/hooks/useTickerSearch.ts
git commit -m "refactor(analysis): drop quote fields from TickerData"
```

---

## Task 12: Write `useQuote` hook (test-first)

**Files:**
- Create: `src/features/analysis/hooks/useQuote.ts`
- Create: `src/features/analysis/hooks/useQuote.test.ts`

- [ ] **Step 1: Define the public surface and write failing tests**

Create `src/features/analysis/hooks/useQuote.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest"
import { renderHook, waitFor, act } from "@testing-library/react"
import { useQuote } from "./useQuote"

const getQuoteMock = vi.fn()
const listenMock = vi.fn()

vi.mock("../../../shared/api/ibkr", () => ({
  ibkrApi: {
    getQuote: (symbol: string) => getQuoteMock(symbol),
  },
}))

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: unknown) => listenMock(event, handler),
}))

const sampleQuote = {
  symbol: "AAPL",
  lastPrice: 202.49,
  prevClose: 200.0,
  volume: 1_234_567,
  timestamp: 1_730_000_000,
}

describe("useQuote", () => {
  beforeEach(() => {
    vi.useFakeTimers()
    getQuoteMock.mockReset()
    listenMock.mockReset()
    listenMock.mockResolvedValue(() => {})
    getQuoteMock.mockResolvedValue(sampleQuote)
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "visible",
    })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it("fetches immediately on mount", async () => {
    const { result } = renderHook(() => useQuote("AAPL"))

    await waitFor(() => {
      expect(getQuoteMock).toHaveBeenCalledWith("AAPL")
    })

    await waitFor(() => {
      expect(result.current.quote).toEqual(sampleQuote)
    })
  })

  it("polls every 5s while visible and connected", async () => {
    renderHook(() => useQuote("AAPL"))

    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(2)

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(3)
  })

  it("does not poll when symbol is null", async () => {
    renderHook(() => useQuote(null))
    await act(async () => {
      vi.advanceTimersByTime(15_000)
    })
    expect(getQuoteMock).not.toHaveBeenCalled()
  })

  it("preserves last good quote across transient errors", async () => {
    getQuoteMock
      .mockResolvedValueOnce(sampleQuote)
      .mockRejectedValueOnce("timeout")
      .mockResolvedValueOnce({ ...sampleQuote, lastPrice: 203.1 })

    const { result } = renderHook(() => useQuote("AAPL"))

    await waitFor(() => expect(result.current.quote?.lastPrice).toBe(202.49))

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    await waitFor(() =>
      expect(result.current.error).toBe("fetch_failed"),
    )
    // last good quote unchanged
    expect(result.current.quote?.lastPrice).toBe(202.49)

    await act(async () => {
      vi.advanceTimersByTime(5_000)
    })
    await waitFor(() => expect(result.current.quote?.lastPrice).toBe(203.1))
    expect(result.current.error).toBeNull()
  })

  it("maps backend error discriminants to QuoteError values", async () => {
    getQuoteMock.mockRejectedValueOnce("disconnected")
    const { result } = renderHook(() => useQuote("AAPL"))
    await waitFor(() => expect(result.current.error).toBe("disconnected"))

    getQuoteMock.mockReset()
    getQuoteMock.mockRejectedValueOnce("no_permission")
    const { result: result2 } = renderHook(() => useQuote("MSFT"))
    await waitFor(() => expect(result2.current.error).toBe("no_permission"))
  })

  it("subscribes to connection-status-changed and stops polling on disconnect", async () => {
    let connectionHandler: ((event: { payload: unknown }) => void) | null = null
    listenMock.mockImplementation((eventName, handler) => {
      if (eventName === "connection-status-changed") {
        connectionHandler = handler as (event: { payload: unknown }) => void
      }
      return Promise.resolve(() => {})
    })

    renderHook(() => useQuote("AAPL"))

    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    // Simulate disconnect event
    act(() => {
      connectionHandler?.({
        payload: { type: "ConnectionStatusChanged", data: { connected: false, message: "down" } },
      })
    })

    await act(async () => {
      vi.advanceTimersByTime(15_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(1) // no new calls

    // Reconnect
    act(() => {
      connectionHandler?.({
        payload: { type: "ConnectionStatusChanged", data: { connected: true, message: "up" } },
      })
    })

    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(2))
  })

  it("pauses polling when the tab is hidden and resumes when visible", async () => {
    renderHook(() => useQuote("AAPL"))
    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(1))

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "hidden",
    })
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"))
    })

    await act(async () => {
      vi.advanceTimersByTime(15_000)
    })
    expect(getQuoteMock).toHaveBeenCalledTimes(1)

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "visible",
    })
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"))
    })

    await waitFor(() => expect(getQuoteMock).toHaveBeenCalledTimes(2))
  })
})
```

- [ ] **Step 2: Run the tests, expect them to fail**

Run: `pnpm vitest run src/features/analysis/hooks/useQuote.test.ts`
Expected: FAIL — "Cannot find module './useQuote'".

- [ ] **Step 3: Implement the hook**

Create `src/features/analysis/hooks/useQuote.ts`:

```ts
import { useEffect, useRef, useState } from "react"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { Quote } from "../../../shared/types"

export type QuoteError =
  | "disconnected"
  | "no_permission"
  | "timeout"
  | "fetch_failed"

interface ConnectionStatusEvent {
  type: "ConnectionStatusChanged"
  data: { connected: boolean; message: string }
}

const POLL_MS = 5_000

function classifyError(raw: unknown): QuoteError {
  const message = typeof raw === "string" ? raw : (raw as Error)?.message ?? ""
  switch (message) {
    case "disconnected":
      return "disconnected"
    case "no_permission":
      return "no_permission"
    case "timeout":
      return "timeout"
    default:
      return "fetch_failed"
  }
}

export function useQuote(symbol: string | null) {
  const [quote, setQuote] = useState<Quote | null>(null)
  const [error, setError] = useState<QuoteError | null>(null)
  const [loading, setLoading] = useState(false)

  // Mutable polling state — kept in refs so the visibility / connection
  // listeners don't capture stale closures across re-renders.
  const intervalRef = useRef<number | null>(null)
  const connectedRef = useRef(true)
  const visibleRef = useRef(true)
  const symbolRef = useRef<string | null>(symbol)

  useEffect(() => {
    symbolRef.current = symbol
  }, [symbol])

  useEffect(() => {
    if (!symbol) {
      setQuote(null)
      setError(null)
      return
    }

    let cancelled = false

    const fetchOnce = async () => {
      const current = symbolRef.current
      if (!current) return
      setLoading(true)
      try {
        const result = await ibkrApi.getQuote(current)
        if (cancelled || symbolRef.current !== current) return
        setQuote(result)
        setError(null)
      } catch (err) {
        if (cancelled || symbolRef.current !== current) return
        setError(classifyError(err))
      } finally {
        if (!cancelled) setLoading(false)
      }
    }

    const startInterval = () => {
      if (intervalRef.current !== null) return
      intervalRef.current = window.setInterval(fetchOnce, POLL_MS)
    }

    const stopInterval = () => {
      if (intervalRef.current !== null) {
        window.clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }

    const ensureRunning = () => {
      if (connectedRef.current && visibleRef.current) {
        startInterval()
      } else {
        stopInterval()
      }
    }

    const onVisibility = () => {
      const wasVisible = visibleRef.current
      visibleRef.current = document.visibilityState === "visible"
      if (!wasVisible && visibleRef.current) {
        // Resumed — fetch immediately, then restart the timer.
        fetchOnce()
      }
      ensureRunning()
    }

    document.addEventListener("visibilitychange", onVisibility)

    let unlistenConnection: UnlistenFn | undefined
    ;(async () => {
      try {
        unlistenConnection = await listen<ConnectionStatusEvent>(
          "connection-status-changed",
          (event) => {
            const wasConnected = connectedRef.current
            connectedRef.current = event.payload.data.connected
            if (!wasConnected && connectedRef.current) {
              fetchOnce()
            }
            if (!connectedRef.current) {
              setError("disconnected")
            }
            ensureRunning()
          },
        )
      } catch (err) {
        console.error("Failed to listen for connection-status-changed:", err)
      }
    })()

    // Reset symbol-scoped state and kick off the first fetch.
    setQuote(null)
    setError(null)
    fetchOnce()
    ensureRunning()

    return () => {
      cancelled = true
      stopInterval()
      document.removeEventListener("visibilitychange", onVisibility)
      unlistenConnection?.()
    }
  }, [symbol])

  return { quote, error, loading }
}
```

- [ ] **Step 4: Run the tests, expect them to pass**

Run: `pnpm vitest run src/features/analysis/hooks/useQuote.test.ts`
Expected: PASS (all 7 tests).

- [ ] **Step 5: Confirm typecheck and lint**

Run:
```
pnpm typecheck
pnpm lint
```
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/features/analysis/hooks/useQuote.ts src/features/analysis/hooks/useQuote.test.ts
git commit -m "feat(analysis): add useQuote hook with 5s polling and connection awareness"
```

---

## Task 13: Update `TickerCards` to consume the live quote

**Files:**
- Modify: `src/features/analysis/components/TickerCards.tsx`

- [ ] **Step 1: Replace `TickerCards` with the four-state implementation**

Replace `src/features/analysis/components/TickerCards.tsx` with:

```tsx
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { TrendingUp, TrendingDown, DollarSign, BarChart3, PieChart, Percent } from "lucide-react"
import type { TickerData } from "../types"
import type { Quote } from "../../../shared/types"
import type { QuoteError } from "../hooks/useQuote"

interface TickerCardsProps {
  ticker: TickerData
  quote: Quote | null
  quoteError: QuoteError | null
}

function formatVolume(volume: number): string {
  return (volume / 1_000_000).toFixed(2) + "M"
}

function quoteStatusMessage(error: QuoteError | null): string | null {
  switch (error) {
    case "disconnected":
      return "Live quote unavailable — TWS not connected"
    case "no_permission":
      return "No live data permission for this symbol"
    case "timeout":
    case "fetch_failed":
      return null // em-dashes only; not worth a UI message for transient errors
    case null:
      return null
  }
}

export function TickerCards({ ticker, quote, quoteError }: TickerCardsProps) {
  const lastPrice = quote?.lastPrice
  const prevClose = quote?.prevClose
  const change =
    lastPrice !== undefined && prevClose !== undefined ? lastPrice - prevClose : undefined
  const changePercent =
    change !== undefined && prevClose !== undefined && prevClose !== 0
      ? (change / prevClose) * 100
      : undefined
  const isPositive = (change ?? 0) >= 0
  const statusMessage = quoteStatusMessage(quoteError)

  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
      {/* Price Card */}
      <Card className="border-border/50 bg-card/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <DollarSign className="h-4 w-4 text-blue-400/60" />
            Current Price
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-foreground text-3xl font-bold">
              {lastPrice !== undefined ? `$${lastPrice.toFixed(2)}` : "—"}
            </p>
            {change !== undefined && changePercent !== undefined && (
              <div
                className={`flex items-center gap-1 text-sm ${isPositive ? "text-green-400" : "text-red-400"}`}
              >
                {isPositive ? (
                  <TrendingUp className="h-4 w-4" />
                ) : (
                  <TrendingDown className="h-4 w-4" />
                )}
                <span>
                  {isPositive ? "+" : ""}
                  {change.toFixed(2)} ({isPositive ? "+" : ""}
                  {changePercent.toFixed(2)}%)
                </span>
              </div>
            )}
            {statusMessage && (
              <p className="text-muted-foreground text-xs">{statusMessage}</p>
            )}
          </div>
        </CardContent>
      </Card>

      {/* Volume Card */}
      <Card className="border-border/50 bg-card/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <BarChart3 className="h-4 w-4 text-purple-400/60" />
            Volume
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-foreground text-3xl font-bold">
              {quote?.volume !== undefined ? formatVolume(quote.volume) : "—"}
            </p>
            <p className="text-muted-foreground text-sm">Trading Volume</p>
          </div>
        </CardContent>
      </Card>

      {/* Market Cap Card */}
      <Card className="border-border/50 bg-card/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <PieChart className="h-4 w-4 text-emerald-400/60" />
            Market Cap
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            <p className="text-foreground text-3xl font-bold">{ticker.marketCap ?? "—"}</p>
            <p className="text-muted-foreground text-sm">{ticker.exchange}</p>
          </div>
        </CardContent>
      </Card>

      {/* Metrics Card */}
      <Card className="border-border/50 bg-card/30 backdrop-blur-xs">
        <CardHeader className="pb-2">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <Percent className="h-4 w-4 text-amber-400/60" />
            Key Metrics
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground text-sm">P/E Ratio</span>
              <span className="text-foreground text-lg font-semibold">
                {ticker.pe?.toFixed(2) ?? "—"}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground text-sm">Yield</span>
              <span className="text-foreground text-lg font-semibold">
                {ticker.yield !== undefined ? ticker.yield.toFixed(2) + "%" : "—"}
              </span>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
```

- [ ] **Step 2: Confirm typecheck and lint**

Run:
```
pnpm typecheck
pnpm lint
```
Expected: PASS. (The component now has new required props; `TickerAnalysis` is updated in Task 14.)

- [ ] **Step 3: Commit**

```bash
git add src/features/analysis/components/TickerCards.tsx
git commit -m "feat(analysis): render live quote with disconnected/no-permission states"
```

---

## Task 14: Wire `useQuote` into `TickerAnalysis`

**Files:**
- Modify: `src/features/analysis/components/TickerAnalysis.tsx`

- [ ] **Step 1: Compose the hook**

Edit `src/features/analysis/components/TickerAnalysis.tsx`:

Add the import near the other hook imports:

```tsx
import { useQuote } from "../hooks/useQuote"
```

Inside the component, after the `useTickerSearch` destructure:

```tsx
  const { quote, error: quoteError } = useQuote(selectedTicker?.symbol || null)
```

Update the `<TickerCards .../>` render call (around line 70):

```tsx
      {selectedTicker && !loading && (
        <TickerCards ticker={selectedTicker} quote={quote} quoteError={quoteError} />
      )}
```

- [ ] **Step 2: Confirm typecheck, lint, build**

Run:
```
pnpm typecheck
pnpm lint
pnpm format
```
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/features/analysis/components/TickerAnalysis.tsx
git commit -m "feat(analysis): wire useQuote into TickerAnalysis"
```

---

## Task 15: Manual smoke test against live TWS

This is a verification-before-completion gate. The `IbkrClient::get_market_data_snapshot` impl has no unit-test coverage, so this is the first time the real TWS path is exercised.

- [ ] **Step 1: Start TWS / IB Gateway** with paper-trading credentials. Confirm it's accepting API connections (default port 7497 for paper).

- [ ] **Step 2: Start the app**

Run: `pnpm tauri dev`
Expected: app launches.

- [ ] **Step 3: Connect to TWS** through the existing UI affordance (whatever the app currently uses to call `ibkr_connect`).

- [ ] **Step 4: Open the Analysis view, search for `AAPL`, select it.**

Expected:
- The fundamentals card (Market Cap / P/E / Yield / Exchange) populates within ~1s.
- Within ~5s, the Current Price card shows a real number that ticks every 5s.
- Volume populates with a `XX.XXM` string.
- Change/changePercent shows a green/red delta below the price.

- [ ] **Step 5: Disconnect TWS** (close it or use the in-app disconnect). 

Expected:
- Polling stops; the price card shows "Live quote unavailable — TWS not connected" within a few seconds.
- Last good price stays visible (em-dashes were the spec; pick whichever the implementation lands on, but verify it matches the spec's description).

- [ ] **Step 6: Reconnect TWS.**

Expected: polling resumes within 5s; the inline message disappears.

- [ ] **Step 7: Hide the window / switch to another tab for >10s, then return.**

Expected: while hidden, no IBKR snapshot calls are made (verifiable via TWS log — look for `reqMktData` requests). On return, an immediate fetch occurs, then polling resumes.

- [ ] **Step 8: If everything passes, no commit needed.** If you hit a regression, that's a real bug — fix it in a new commit before claiming done.

---

## Self-Review Notes

After writing the plan, ran the self-review checklist:

**Spec coverage:** every spec section maps to a task.
- "Stop aliasing 52WeekHigh as price" → Task 8
- "New `Quote` type" → Task 2
- "`QuoteFetcher` trait + `QuoteService`" → Tasks 3, 4, 6
- "Real `IbkrClient::get_market_data_snapshot`" → Task 5
- "`MarketDataPermissionDenied` typed variant" → Task 1
- "Tauri command `ibkr_get_quote`" → Task 7
- "Wire into `lib.rs`" → Task 9
- "TS `Quote` + API wrapper" → Task 10
- "Trim `TickerData`" → Task 11
- "`useQuote` hook with polling, visibility, disconnect" → Task 12
- "`TickerCards` four states" → Task 13
- "Compose into `TickerAnalysis`" → Task 14
- Manual smoke (TDD-discipline note in spec was about backend; the live-TWS path remains manual) → Task 15

**Placeholder scan:** none. Every step has either exact code, an exact command, or an exact verification step.

**Type consistency:** `Quote { symbol, last_price, prev_close, volume, timestamp }` is the same in Tasks 2, 3, 7, 10. `QuoteError` is the same in Tasks 7 (backend mapping) and 12 (frontend type). `connection-status-changed` event payload `{ type, data: { connected, message } }` matches `events/emitter.rs:14`.

**Notes / known gotchas:**
- Task 5 has a fallback note for `TickType::Delayed*` variants that may not exist in the installed `ibapi` version — drop those arms if they don't compile.
- Task 5 ignores `apply_price_size`'s `size_tick_type` for some cases; that's fine because the volume tick comes through as a plain `TickSize`, not `TickPriceSize`.
- The 7-day fundamentals cache is unchanged. If a user has a cached `FundamentalData` from before this fix, their `currentMetrics.price` field will deserialize to `None` (the field is now `#[serde(skip_serializing_if = "Option::is_none")]` and the deserializer is forgiving when missing) — no migration needed.
