//! [`ShadowingNewsProvider`] — Phase 8 cutover shadow comparator.
//!
//! Wraps the production IBKR news provider, fires the AV provider in
//! the background on every fetch, and logs coverage diffs so the user
//! can decide (per `loop/plan/master.md` Phase 8) whether IBKR coverage
//! is materially thinner than AV before the deletion commit lands.
//!
//! Hard rules:
//!
//! - The wrapped IBKR call's result is returned synchronously and
//!   unmodified. The shadow comparison MUST NOT block, fail, or
//!   otherwise affect the primary path.
//! - AV failures during shadowing are swallowed and logged as
//!   `AV unavailable, no comparison`. The plan calls this out
//!   explicitly under "Open risks → Shadow comparison cost".
//! - Active only when both `shadow_av_news_comparison: true` AND an
//!   `ALPHA_VANTAGE_API_KEY` is configured. `lib.rs` enforces both
//!   gates; this wrapper assumes it is only constructed when active.
//!
//! Deleted alongside the Phase 8 deletion commit.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use crate::ibkr::types::news::NewsItem;

use super::{NewsError, NewsProvider};

/// Threshold below which IBKR coverage is logged as a "material gap"
/// (per `phase-8-av-deletion.md` § "Decisions to make in this phase":
/// IBKR returning ≥80% of AV's items counts as parity).
const MATERIAL_GAP_RATIO: f64 = 0.80;

pub struct ShadowingNewsProvider {
    primary: Arc<dyn NewsProvider>,
    shadow: Arc<dyn NewsProvider>,
}

impl ShadowingNewsProvider {
    pub fn new(primary: Arc<dyn NewsProvider>, shadow: Arc<dyn NewsProvider>) -> Self {
        Self { primary, shadow }
    }
}

#[async_trait]
impl NewsProvider for ShadowingNewsProvider {
    async fn fetch(&self, symbol: &str, lookback_hours: u32) -> Result<Vec<NewsItem>, NewsError> {
        let primary_started = Instant::now();
        let primary_result = self.primary.fetch(symbol, lookback_hours).await;
        let primary_elapsed = primary_started.elapsed();

        let primary_count = primary_result.as_ref().map(|v| v.len()).unwrap_or(0);
        let shadow = Arc::clone(&self.shadow);
        let symbol_owned = symbol.to_string();
        tokio::spawn(async move {
            let shadow_started = Instant::now();
            match shadow.fetch(&symbol_owned, lookback_hours).await {
                Ok(shadow_items) => {
                    let report = ShadowReport::compute(primary_count, shadow_items.len());
                    tracing::info!(
                        symbol = %symbol_owned,
                        lookback_hours,
                        primary_count,
                        shadow_count = shadow_items.len(),
                        coverage_ratio = report.coverage_ratio,
                        material_gap = report.material_gap,
                        primary_ms = primary_elapsed.as_millis() as u64,
                        shadow_ms = shadow_started.elapsed().as_millis() as u64,
                        "shadow_news_comparison"
                    );
                }
                Err(e) => {
                    tracing::info!(
                        symbol = %symbol_owned,
                        lookback_hours,
                        error = %e,
                        "shadow_news_comparison: AV unavailable, no comparison"
                    );
                }
            }
        });

        primary_result
    }
}

/// Pure-data summary of one shadow comparison. Pulled out of `fetch`
/// so the diff math is unit-testable without spawning Tokio tasks or
/// inspecting log output.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ShadowReport {
    coverage_ratio: f64,
    material_gap: bool,
}

impl ShadowReport {
    fn compute(primary_count: usize, shadow_count: usize) -> Self {
        // Convention: when AV (shadow) returned zero, the IBKR side has
        // nothing to under-cover, so ratio = 1.0 (no gap). Avoids a NaN
        // and keeps "material_gap" meaning "IBKR returned far fewer
        // items than AV did" rather than triggering on the empty-case.
        let coverage_ratio = if shadow_count == 0 {
            1.0
        } else {
            (primary_count as f64) / (shadow_count as f64)
        };
        let material_gap = coverage_ratio < MATERIAL_GAP_RATIO;
        Self {
            coverage_ratio,
            material_gap,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;

    use crate::services::news_provider::test_support::FakeNewsProvider;

    use super::*;

    fn news_item(title: &str) -> NewsItem {
        NewsItem {
            time_published: Utc::now(),
            title: title.to_string(),
            summary: String::new(),
            source: "shadow-test".to_string(),
            url: String::new(),
            overall_sentiment_score: None,
            overall_sentiment_label: None,
            ticker_sentiment: Vec::new(),
        }
    }

    #[test]
    fn report_parity_when_counts_match() {
        let r = ShadowReport::compute(10, 10);
        assert!((r.coverage_ratio - 1.0).abs() < f64::EPSILON);
        assert!(!r.material_gap);
    }

    #[test]
    fn report_material_gap_when_under_eighty_percent() {
        let r = ShadowReport::compute(7, 10); // 70%
        assert!(r.material_gap);
    }

    #[test]
    fn report_no_gap_at_threshold() {
        let r = ShadowReport::compute(8, 10); // exactly 80%
        assert!(!r.material_gap);
    }

    #[test]
    fn report_handles_zero_shadow_without_nan() {
        let r = ShadowReport::compute(5, 0);
        assert!((r.coverage_ratio - 1.0).abs() < f64::EPSILON);
        assert!(!r.material_gap);
    }

    /// Counting wrapper around `FakeNewsProvider` so the test can
    /// assert the shadow side was actually invoked.
    struct CountingProvider {
        inner: Arc<FakeNewsProvider>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl NewsProvider for CountingProvider {
        async fn fetch(
            &self,
            symbol: &str,
            lookback_hours: u32,
        ) -> Result<Vec<NewsItem>, NewsError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.fetch(symbol, lookback_hours).await
        }
    }

    #[tokio::test]
    async fn primary_result_returned_unchanged() {
        let primary = FakeNewsProvider::new();
        primary.insert("AAPL", vec![news_item("p1"), news_item("p2")]);
        let shadow = FakeNewsProvider::new();
        shadow.insert("AAPL", vec![news_item("s1")]);

        let wrapper = ShadowingNewsProvider::new(Arc::new(primary), Arc::new(shadow));
        let got = wrapper.fetch("AAPL", 24).await.expect("primary ok");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "p1");
    }

    #[tokio::test]
    async fn shadow_is_invoked_in_background() {
        let primary = Arc::new(FakeNewsProvider::new());
        primary.insert("AAPL", vec![news_item("p1")]);

        let shadow_inner = Arc::new(FakeNewsProvider::new());
        shadow_inner.insert("AAPL", vec![news_item("s1")]);
        let shadow_calls = Arc::new(AtomicUsize::new(0));
        let shadow = CountingProvider {
            inner: Arc::clone(&shadow_inner),
            calls: Arc::clone(&shadow_calls),
        };

        let wrapper = ShadowingNewsProvider::new(primary, Arc::new(shadow));
        wrapper.fetch("AAPL", 24).await.expect("primary ok");

        // Background task is spawned via `tokio::spawn`; yield until it
        // observes the call. Bounded loop so the test fails fast rather
        // than hangs if the spawn is dropped.
        for _ in 0..50 {
            if shadow_calls.load(Ordering::SeqCst) == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!(
            "shadow provider was not invoked within yield budget (calls = {})",
            shadow_calls.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn shadow_failure_does_not_affect_primary_result() {
        let primary = FakeNewsProvider::new();
        primary.insert("AAPL", vec![news_item("p1")]);
        let shadow = FakeNewsProvider::new();
        shadow.fail_with("simulated AV blowup");

        let wrapper = ShadowingNewsProvider::new(Arc::new(primary), Arc::new(shadow));
        let got = wrapper.fetch("AAPL", 24).await.expect("primary still ok");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "p1");
    }

    #[tokio::test]
    async fn primary_error_propagates() {
        let primary = FakeNewsProvider::new();
        primary.fail_with("upstream IBKR error");
        let shadow = FakeNewsProvider::new();

        let wrapper = ShadowingNewsProvider::new(Arc::new(primary), Arc::new(shadow));
        let err = wrapper.fetch("AAPL", 24).await.expect_err("propagates");
        assert!(matches!(err, NewsError::Other(_)));
    }
}
