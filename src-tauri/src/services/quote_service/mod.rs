use std::sync::Arc;

use async_trait::async_trait;

use crate::ibkr::error::Result;
use crate::ibkr::types::{MarketDataSnapshot, Quote};

/// Narrow IBKR seam for the live-quote path. Mirrors the
/// `HistoricalDataFetcher` / `MarketScanner` pattern: the real client
/// implements this trait inherently, the mock implements it via
/// `MockIbkrClient`, and tests only ever depend on this trait.
#[async_trait]
pub trait QuoteFetcher: Send + Sync {
    async fn get_market_data_snapshot(&self, symbol: &str) -> Result<MarketDataSnapshot>;
}

#[allow(dead_code)] // wired in lib.rs once Tauri commands land
pub struct QuoteService {
    fetcher: Arc<dyn QuoteFetcher>,
}

impl QuoteService {
    #[allow(dead_code)] // wired in lib.rs once Tauri commands land
    pub fn new(fetcher: Arc<dyn QuoteFetcher>) -> Self {
        Self { fetcher }
    }

    /// Fetches a `MarketDataSnapshot` and projects it into a `Quote`.
    /// Errors propagate untranslated — the Tauri command layer maps
    /// them to user-facing strings.
    #[allow(dead_code)] // wired in lib.rs once Tauri commands land
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

    use std::sync::Mutex;

    use crate::ibkr::error::IbkrError;

    /// Programmable mock for `QuoteFetcher`. Tests inject either a
    /// canned snapshot or a canned error.
    ///
    /// Single-shot: instantiate one stub per service call. The internal
    /// `Mutex<Option<...>>::take()` panics if `get_market_data_snapshot`
    /// is invoked twice on the same stub.
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
}
