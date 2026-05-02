//! Trait-shape tests for [`FundamentalsProvider`]:
//!
//! - Dyn-compatibility: `Arc<dyn FundamentalsProvider>` round-trips
//!   through `Send + Sync + 'static` bounds (compile-time).
//! - Fake round-trip: `FakeFundamentalsProvider::insert` surfaces via
//!   the trait method.
//! - AV-adapter round-trip via the existing `AvHttp` mock seam: a canned
//!   AV payload set surfaces through the trait identical to what the
//!   pre-Phase-3 `FinancialDataService::fetch_fundamental_data` returned.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::ibkr::types::{CurrentMetrics, FundamentalData, HistoricalFinancial};
use crate::services::cache_service::CacheService;
use crate::services::financial_data_service::{AvHttp, AvHttpError, FinancialDataService};

use super::alpha_vantage::AlphaVantageFundamentalsProvider;
use super::test_support::FakeFundamentalsProvider;
use super::{FundamentalsError, FundamentalsProvider};

/// Compile-time check: `Arc<dyn FundamentalsProvider>` satisfies the
/// trait bounds Tauri's `app.manage` requires. Failing to compile this
/// is the regression we care about, not the runtime call.
#[test]
fn arc_dyn_provider_is_send_sync_static() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<Arc<dyn FundamentalsProvider>>();
}

#[tokio::test]
async fn fake_provider_returns_inserted_row() {
    let fake = FakeFundamentalsProvider::new();
    let data = FundamentalData {
        symbol: "AAPL".to_string(),
        historical: vec![HistoricalFinancial {
            year: 2024,
            revenue: 390.0,
            net_income: 100.0,
            eps: 6.5,
        }],
        analyst_estimates: None,
        current_metrics: CurrentMetrics {
            price: None,
            pe_ratio: 30.0,
            shares_outstanding: 15_000.0,
            name: Some("Apple Inc.".into()),
            exchange: Some("NASDAQ".into()),
            market_cap: Some("3000000000000".into()),
            dividend_yield: Some(0.005),
        },
    };
    fake.insert("AAPL", data.clone());

    let provider: Arc<dyn FundamentalsProvider> = Arc::new(fake);
    let got = provider.fetch("aapl").await.expect("hit");
    assert_eq!(got.symbol, data.symbol);
    assert_eq!(got.historical.len(), 1);
    assert_eq!(got.historical[0].revenue, 390.0);
    assert_eq!(got.current_metrics.pe_ratio, 30.0);
}

#[tokio::test]
async fn fake_provider_unknown_symbol_returns_not_found() {
    let fake = FakeFundamentalsProvider::new();
    let provider: Arc<dyn FundamentalsProvider> = Arc::new(fake);
    let err = provider.fetch("XYZ").await.expect_err("must error");
    assert!(matches!(err, FundamentalsError::NotFound(s) if s == "XYZ"));
}

#[tokio::test]
async fn fake_provider_force_error_surfaces_as_other() {
    let fake = FakeFundamentalsProvider::new();
    fake.fail_with("simulated upstream blowup");
    let provider: Arc<dyn FundamentalsProvider> = Arc::new(fake);
    let err = provider.fetch("AAPL").await.expect_err("must error");
    let msg = err.to_string();
    assert!(msg.contains("simulated upstream blowup"), "got: {msg}");
    assert!(matches!(err, FundamentalsError::Other(_)));
}

// ---------- AV adapter round-trip via mock HTTP ----------

struct CountingFakeAv {
    counter: Arc<AtomicUsize>,
    overview: Value,
    income: Value,
    earnings: Value,
}

#[async_trait]
impl AvHttp for CountingFakeAv {
    async fn fetch(&self, url: &str) -> Result<Value, AvHttpError> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        Ok(if url.contains("function=OVERVIEW") {
            self.overview.clone()
        } else if url.contains("function=INCOME_STATEMENT") {
            self.income.clone()
        } else if url.contains("function=EARNINGS") {
            self.earnings.clone()
        } else {
            return Err(AvHttpError::Status(format!("unexpected url: {url}")));
        })
    }
}

#[tokio::test]
async fn av_adapter_round_trips_canned_payloads_into_fundamental_data() {
    let temp = TempDir::new().expect("tempdir");
    let cache = CacheService::new(temp.path()).expect("cache");
    let counter = Arc::new(AtomicUsize::new(0));
    let fake = Arc::new(CountingFakeAv {
        counter: Arc::clone(&counter),
        overview: json!({
            "Symbol": "AAPL",
            "Name": "Apple Inc.",
            "Exchange": "NASDAQ",
            "MarketCapitalization": "3000000000000",
            "PERatio": "30.0",
            "SharesOutstanding": "15000000000",
            "DividendYield": "0.005"
        }),
        income: json!({
            "symbol": "AAPL",
            "annualReports": [
                {"fiscalDateEnding": "2024-09-30", "totalRevenue": "390000000000", "netIncome": "100000000000"},
                {"fiscalDateEnding": "2023-09-30", "totalRevenue": "380000000000", "netIncome": "90000000000"}
            ]
        }),
        earnings: json!({
            "symbol": "AAPL",
            "annualEarnings": [
                {"fiscalDateEnding": "2024-09-30", "reportedEPS": "6.5"},
                {"fiscalDateEnding": "2023-09-30", "reportedEPS": "6.0"}
            ],
            "quarterlyEarnings": []
        }),
    });
    let svc = Arc::new(
        FinancialDataService::new("KEY".to_string())
            .with_http(fake.clone() as Arc<dyn AvHttp>)
            .with_cache(cache),
    );
    let provider = AlphaVantageFundamentalsProvider::new(svc, true);

    let data = provider.fetch("AAPL").await.expect("ok");
    assert_eq!(data.symbol, "AAPL");
    assert!(!data.historical.is_empty(), "historical must be populated");
    assert_eq!(data.current_metrics.pe_ratio, 30.0);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        3,
        "fan-out hits 3 endpoints"
    );
}

/// Always-Information fake. The wrapped service classifies this as a
/// soft-skip; with no cache row available it propagates as an error,
/// which the adapter must classify as `RateLimited`.
struct InformationFakeAv;

#[async_trait]
impl AvHttp for InformationFakeAv {
    async fn fetch(&self, _url: &str) -> Result<Value, AvHttpError> {
        Ok(json!({
            "Information": "Thank you for using Alpha Vantage! Our standard API rate limit is 25 requests per day."
        }))
    }
}

#[tokio::test]
async fn av_adapter_classifies_rate_limit_response_as_rate_limited() {
    let temp = TempDir::new().expect("tempdir");
    let cache = CacheService::new(temp.path()).expect("cache");
    let svc = Arc::new(
        FinancialDataService::new("KEY".to_string())
            .with_http(Arc::new(InformationFakeAv) as Arc<dyn AvHttp>)
            .with_cache(cache),
    );
    let provider = AlphaVantageFundamentalsProvider::new(svc, true);

    let err = provider.fetch("AAPL").await.expect_err("must error");
    assert!(
        matches!(err, FundamentalsError::RateLimited { .. }),
        "expected RateLimited, got: {err:?}"
    );
}
