//! [`CompositeFundamentalsProvider`] — the production
//! [`FundamentalsProvider`] wired into `lib.rs` after Phase 4. Reads in
//! order:
//!
//! 1. **Manual store.** Operator-curated rows written by the MCP
//!    `set_fundamentals` tool. Always wins (Hard Invariant #8).
//! 2. **AV adapter.** The Phase 3 [`super::alpha_vantage::AlphaVantageFundamentalsProvider`].
//!    Internally honours the AV file cache, in-flight coalescing, and
//!    the stale-cache fallback so we don't re-implement those layers.
//!
//! The composite logs the chosen source at info level so production
//! traces show whether a `get_fundamentals` call landed on manual data
//! or fell through to AV.
//!
//! Hard Invariant #6 (the tracker doesn't fetch fundamentals) is
//! preserved: this provider only adds layering — every consumer is still
//! a user-explicit code path (analysis UI, MCP tools).

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, warn};

use crate::ibkr::types::FundamentalData;

use super::manual::ManualFundamentalsProvider;
use super::{FundamentalsError, FundamentalsProvider};

/// Provider composition: manual store first, AV second. Both arms are
/// `Arc<dyn FundamentalsProvider>` so tests can swap fakes for either
/// layer without touching the composite logic.
pub struct CompositeFundamentalsProvider {
    manual: Arc<ManualFundamentalsProvider>,
    av: Arc<dyn FundamentalsProvider>,
}

impl CompositeFundamentalsProvider {
    pub fn new(manual: Arc<ManualFundamentalsProvider>, av: Arc<dyn FundamentalsProvider>) -> Self {
        Self { manual, av }
    }
}

#[async_trait]
impl FundamentalsProvider for CompositeFundamentalsProvider {
    async fn fetch(&self, symbol: &str) -> Result<FundamentalData, FundamentalsError> {
        match self.manual.fetch(symbol).await {
            Ok(data) => {
                info!("fundamentals(manual): served {symbol} from operator-curated manual store");
                Ok(data)
            }
            Err(FundamentalsError::NotFound(_)) => {
                info!(
                    "fundamentals(av): manual store empty for {symbol}, falling through to Alpha Vantage"
                );
                self.av.fetch(symbol).await
            }
            Err(e) => {
                // Manual store I/O blew up — distinct from "no row".
                // Don't paper over with the AV fallback because we'd
                // mask a SQLite/serde bug; surface the error and let
                // the operator triage.
                warn!("fundamentals(manual): store read failed for {symbol}: {e}");
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::NamedTempFile;

    use super::*;
    use crate::ibkr::types::{CurrentMetrics, FundamentalData, HistoricalFinancial};
    use crate::services::fundamentals_provider::test_support::FakeFundamentalsProvider;
    use crate::services::manual_fundamentals_store::ManualFundamentalsStore;
    use crate::storage::Db;

    fn fd(symbol: &str, pe: f64) -> FundamentalData {
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
                pe_ratio: pe,
                shares_outstanding: 1_000.0,
                name: None,
                exchange: None,
                market_cap: None,
                dividend_yield: None,
            },
        }
    }

    fn fresh_store() -> (NamedTempFile, Arc<ManualFundamentalsStore>) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Db::open(tmp.path()).unwrap());
        (tmp, Arc::new(ManualFundamentalsStore::new(db)))
    }

    #[tokio::test]
    async fn empty_manual_falls_through_to_av() {
        let (_tmp, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let av = FakeFundamentalsProvider::new();
        av.insert("AAPL", fd("AAPL", 30.0));
        let av: Arc<dyn FundamentalsProvider> = Arc::new(av);
        let composite = CompositeFundamentalsProvider::new(manual, av);
        let got = composite.fetch("AAPL").await.unwrap();
        assert_eq!(got.current_metrics.pe_ratio, 30.0);
    }

    #[tokio::test]
    async fn manual_row_wins_over_av() {
        let (_tmp, store) = fresh_store();
        store
            .upsert(
                "AAPL",
                fd("AAPL", 99.0),
                "2026-05-02",
                "src",
                "interactive",
                0,
            )
            .await
            .unwrap();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let av_fake = FakeFundamentalsProvider::new();
        av_fake.insert("AAPL", fd("AAPL", 30.0));
        let av: Arc<dyn FundamentalsProvider> = Arc::new(av_fake);
        let composite = CompositeFundamentalsProvider::new(manual, av);
        let got = composite.fetch("AAPL").await.unwrap();
        assert_eq!(
            got.current_metrics.pe_ratio, 99.0,
            "manual store must win over AV"
        );
    }

    #[tokio::test]
    async fn empty_manual_propagates_av_not_found() {
        let (_tmp, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let av: Arc<dyn FundamentalsProvider> = Arc::new(FakeFundamentalsProvider::new());
        let composite = CompositeFundamentalsProvider::new(manual, av);
        let err = composite.fetch("NOPE").await.expect_err("must error");
        assert!(matches!(err, FundamentalsError::NotFound(_)));
    }

    #[tokio::test]
    async fn empty_manual_propagates_av_other_errors_unchanged() {
        let (_tmp, store) = fresh_store();
        let manual = Arc::new(ManualFundamentalsProvider::new(store));
        let av_fake = FakeFundamentalsProvider::new();
        av_fake.fail_with("AV transport blew up");
        let av: Arc<dyn FundamentalsProvider> = Arc::new(av_fake);
        let composite = CompositeFundamentalsProvider::new(manual, av);
        let err = composite.fetch("AAPL").await.expect_err("must error");
        let msg = err.to_string();
        assert!(msg.contains("AV transport blew up"), "got: {msg}");
    }

    #[tokio::test]
    async fn manual_overwrite_changes_returned_value() {
        let (_tmp, store) = fresh_store();
        store
            .upsert(
                "MSFT",
                fd("MSFT", 25.0),
                "2026-04-01",
                "v1",
                "interactive",
                0,
            )
            .await
            .unwrap();
        let manual = Arc::new(ManualFundamentalsProvider::new(Arc::clone(&store)));
        let av: Arc<dyn FundamentalsProvider> = Arc::new(FakeFundamentalsProvider::new());
        let composite = CompositeFundamentalsProvider::new(manual, av);
        assert_eq!(
            composite
                .fetch("MSFT")
                .await
                .unwrap()
                .current_metrics
                .pe_ratio,
            25.0
        );
        store
            .upsert(
                "MSFT",
                fd("MSFT", 35.0),
                "2026-05-02",
                "v2",
                "interactive",
                1,
            )
            .await
            .unwrap();
        assert_eq!(
            composite
                .fetch("MSFT")
                .await
                .unwrap()
                .current_metrics
                .pe_ratio,
            35.0,
        );
    }
}
