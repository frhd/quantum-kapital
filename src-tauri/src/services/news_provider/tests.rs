//! Trait-shape tests for [`NewsProvider`]:
//!
//! - Dyn-compatibility: `Arc<dyn NewsProvider>` round-trips through
//!   `Send + Sync + 'static` bounds (compile-time).
//! - Fake round-trip: [`FakeNewsProvider::insert`] surfaces via the
//!   trait method.
//! - Empty-symbol-row returns `Ok(Vec::new())` (the canonical "no news"
//!   signal, NOT an error).
//! - Forced error surfaces as [`NewsError::Other`].
//!
//! Adapter-level classifier tests live in
//! [`super::alpha_vantage::adapter_tests`]. End-to-end round-trip
//! through `FinancialDataService` waits on Phase 7 part B's IBKR
//! provider, where the integration shape is exercised against fixtures.

use std::sync::Arc;

use chrono::Utc;

use crate::ibkr::types::news::NewsItem;

use super::test_support::FakeNewsProvider;
use super::{NewsError, NewsProvider};

/// Compile-time check: `Arc<dyn NewsProvider>` satisfies the trait
/// bounds Tauri's `app.manage` requires. Failing to compile this is
/// the regression we care about, not the runtime call.
#[test]
fn arc_dyn_provider_is_send_sync_static() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<Arc<dyn NewsProvider>>();
}

#[tokio::test]
async fn fake_provider_returns_inserted_rows() {
    let fake = FakeNewsProvider::new();
    let items = vec![NewsItem {
        time_published: Utc::now(),
        title: "Apple beats earnings".to_string(),
        summary: "Q4 numbers above consensus.".to_string(),
        source: "Reuters".to_string(),
        url: "https://example.com/aapl-q4".to_string(),
        overall_sentiment_score: Some(0.45),
        overall_sentiment_label: Some("Bullish".to_string()),
        ticker_sentiment: Vec::new(),
    }];
    fake.insert("AAPL", items.clone());

    let provider: Arc<dyn NewsProvider> = Arc::new(fake);
    let got = provider.fetch("aapl", 24).await.expect("hit");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].title, "Apple beats earnings");
    assert_eq!(got[0].source, "Reuters");
}

#[tokio::test]
async fn fake_provider_unknown_symbol_returns_empty_not_error() {
    let fake = FakeNewsProvider::new();
    let provider: Arc<dyn NewsProvider> = Arc::new(fake);
    let got = provider.fetch("XYZ", 24).await.expect("Ok(empty)");
    assert!(
        got.is_empty(),
        "unknown symbol must return Ok(Vec::new()) — see trait docs"
    );
}

#[tokio::test]
async fn fake_provider_force_error_surfaces_as_other() {
    let fake = FakeNewsProvider::new();
    fake.fail_with("simulated upstream blowup");
    let provider: Arc<dyn NewsProvider> = Arc::new(fake);
    let err = provider.fetch("AAPL", 24).await.expect_err("must error");
    let msg = err.to_string();
    assert!(msg.contains("simulated upstream blowup"), "got: {msg}");
    assert!(matches!(err, NewsError::Other(_)));
}
