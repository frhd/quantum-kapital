//! [`ManualFundamentalsProvider`] — thin adapter over
//! [`ManualFundamentalsStore`] implementing the [`FundamentalsProvider`]
//! trait. Lets the manual store sit behind the same trait surface as the
//! AV adapter so the composite provider can fall through to AV when the
//! store is empty for a symbol.
//!
//! The store always wins when present (Hard Invariant #8 in
//! `loop/plan/master.md`). Empty / missing rows surface as
//! [`FundamentalsError::NotFound`] so the composite can detect "no
//! manual entry" and try the next layer without coupling to the store
//! type directly.

use std::sync::Arc;

use async_trait::async_trait;

use crate::ibkr::types::FundamentalData;
use crate::services::manual_fundamentals_store::ManualFundamentalsStore;

use super::{FundamentalsError, FundamentalsProvider};

pub struct ManualFundamentalsProvider {
    store: Arc<ManualFundamentalsStore>,
}

impl ManualFundamentalsProvider {
    pub fn new(store: Arc<ManualFundamentalsStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl FundamentalsProvider for ManualFundamentalsProvider {
    async fn fetch(&self, symbol: &str) -> Result<FundamentalData, FundamentalsError> {
        let key = symbol.trim().to_uppercase();
        match self.store.get(&key).await {
            Ok(Some(row)) => Ok(row.data),
            Ok(None) => Err(FundamentalsError::NotFound(key)),
            Err(e) => Err(FundamentalsError::Other(format!(
                "manual fundamentals store read failed: {e}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::NamedTempFile;

    use super::*;
    use crate::ibkr::types::{CurrentMetrics, FundamentalData, HistoricalFinancial};
    use crate::storage::Db;

    fn open_store() -> (NamedTempFile, Arc<ManualFundamentalsStore>) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, Arc::new(ManualFundamentalsStore::new(db)))
    }

    fn sample(symbol: &str) -> FundamentalData {
        FundamentalData {
            symbol: symbol.to_string(),
            historical: vec![HistoricalFinancial {
                year: 2024,
                revenue: 100.0,
                net_income: 10.0,
                eps: 1.0,
            }],
            analyst_estimates: None,
            current_metrics: CurrentMetrics {
                price: None,
                pe_ratio: 12.5,
                shares_outstanding: 1_000.0,
                name: None,
                exchange: None,
                market_cap: None,
                dividend_yield: None,
            },
        }
    }

    #[tokio::test]
    async fn returns_inserted_payload() {
        let (_tmp, store) = open_store();
        store
            .upsert("AAPL", sample("AAPL"), "2026-05-02", "src", "interactive", 0)
            .await
            .unwrap();
        let provider = ManualFundamentalsProvider::new(store);
        let got = provider.fetch("aapl").await.unwrap();
        assert_eq!(got.symbol, "AAPL");
        assert_eq!(got.current_metrics.pe_ratio, 12.5);
    }

    #[tokio::test]
    async fn empty_store_yields_not_found() {
        let (_tmp, store) = open_store();
        let provider = ManualFundamentalsProvider::new(store);
        let err = provider.fetch("ZZZZ").await.expect_err("must error");
        assert!(matches!(err, FundamentalsError::NotFound(s) if s == "ZZZZ"));
    }
}
