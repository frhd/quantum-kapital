//! Round-trip tests for [`ManualFundamentalsStore`]. The store holds
//! the operator-curated manual rows the MCP `set_fundamentals` tool
//! writes, so the assertions below pin the contract the tool relies on:
//!
//! - Symbol normalisation (case-insensitive, trimmed).
//! - `upsert` returns the prior row on overwrite (the MCP tool diffs
//!   `prior` vs `current` in its response).
//! - `clear` removes a row without erroring on a missing row.
//! - `list_with_freshness` returns newest-first.

use std::sync::Arc;

use tempfile::NamedTempFile;

use super::ManualFundamentalsStore;
use crate::ibkr::types::{
    AnalystEstimate, AnalystEstimates, CurrentMetrics, FundamentalData, HistoricalFinancial,
};
use crate::storage::Db;

fn open_store() -> (NamedTempFile, ManualFundamentalsStore) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Arc::new(Db::open(tmp.path()).expect("open db"));
    (tmp, ManualFundamentalsStore::new(db))
}

fn sample_fd(symbol: &str) -> FundamentalData {
    FundamentalData {
        symbol: symbol.to_string(),
        historical: vec![HistoricalFinancial {
            year: 2024,
            revenue: 391.0,
            net_income: 99.8,
            eps: 6.5,
        }],
        analyst_estimates: Some(AnalystEstimates {
            revenue: vec![AnalystEstimate {
                year: 2025,
                estimate: 420.0,
            }],
            eps: vec![AnalystEstimate {
                year: 2025,
                estimate: 7.1,
            }],
        }),
        current_metrics: CurrentMetrics {
            price: None,
            pe_ratio: 30.0,
            shares_outstanding: 15_500.0,
            name: Some("Apple Inc.".into()),
            exchange: Some("NASDAQ".into()),
            market_cap: Some("3000000000000".into()),
            dividend_yield: Some(0.005),
        },
    }
}

#[tokio::test]
async fn upsert_then_get_round_trips_payload() {
    let (_tmp, store) = open_store();
    let data = sample_fd("AAPL");
    let outcome = store
        .upsert(
            "aapl",
            data.clone(),
            "2026-05-02",
            "Bloomberg paste",
            "interactive",
            1_700_000_000,
        )
        .await
        .expect("upsert");
    assert_eq!(outcome.current.symbol, "AAPL");
    assert!(outcome.prior.is_none(), "prior must be None on first write");
    assert_eq!(outcome.current.data.current_metrics.pe_ratio, 30.0);

    let row = store.get("AAPL").await.expect("get").expect("present");
    assert_eq!(row.symbol, "AAPL");
    assert_eq!(row.as_of_date, "2026-05-02");
    assert_eq!(row.source, "Bloomberg paste");
    assert_eq!(row.data.historical.len(), 1);
    assert_eq!(row.data.historical[0].revenue, 391.0);
}

#[tokio::test]
async fn upsert_returns_prior_row_on_overwrite() {
    let (_tmp, store) = open_store();
    let original = sample_fd("AAPL");
    store
        .upsert(
            "AAPL",
            original.clone(),
            "2026-04-01",
            "v1",
            "interactive",
            1_700_000_000,
        )
        .await
        .unwrap();

    let mut updated = sample_fd("AAPL");
    updated.current_metrics.pe_ratio = 35.0;
    let outcome = store
        .upsert(
            "AAPL",
            updated.clone(),
            "2026-05-02",
            "v2",
            "interactive",
            1_700_086_400,
        )
        .await
        .unwrap();
    let prior = outcome.prior.expect("prior row returned on overwrite");
    assert_eq!(prior.source, "v1");
    assert_eq!(prior.data.current_metrics.pe_ratio, 30.0);
    assert_eq!(outcome.current.source, "v2");
    assert_eq!(outcome.current.data.current_metrics.pe_ratio, 35.0);
}

#[tokio::test]
async fn get_missing_symbol_returns_none() {
    let (_tmp, store) = open_store();
    assert!(store.get("ZZZZ").await.unwrap().is_none());
}

#[tokio::test]
async fn get_normalises_case_and_whitespace() {
    let (_tmp, store) = open_store();
    let data = sample_fd("AAPL");
    store
        .upsert(
            "AAPL",
            data,
            "2026-05-02",
            "src",
            "interactive",
            1_700_000_000,
        )
        .await
        .unwrap();
    let row = store.get(" aapl ").await.unwrap().expect("hit");
    assert_eq!(row.symbol, "AAPL");
}

#[tokio::test]
async fn clear_removes_row_and_is_idempotent() {
    let (_tmp, store) = open_store();
    let data = sample_fd("MSFT");
    store
        .upsert(
            "MSFT",
            data,
            "2026-05-02",
            "src",
            "interactive",
            1_700_000_000,
        )
        .await
        .unwrap();
    store.clear("MSFT").await.unwrap();
    assert!(store.get("MSFT").await.unwrap().is_none());
    // Second clear is a no-op.
    store.clear("MSFT").await.unwrap();
}

#[tokio::test]
async fn list_with_freshness_orders_newest_first() {
    let (_tmp, store) = open_store();
    store
        .upsert(
            "OLD",
            sample_fd("OLD"),
            "2026-01-01",
            "src",
            "interactive",
            1_700_000_000,
        )
        .await
        .unwrap();
    store
        .upsert(
            "NEW",
            sample_fd("NEW"),
            "2026-05-02",
            "src",
            "interactive",
            1_700_086_400,
        )
        .await
        .unwrap();
    let listing = store.list_with_freshness().await.unwrap();
    assert_eq!(listing.len(), 2);
    assert_eq!(listing[0].0, "NEW");
    assert_eq!(listing[1].0, "OLD");
}
