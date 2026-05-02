//! [`AlphaVantageNewsProvider`] — the Phase 7 part-A [`NewsProvider`]
//! impl. Thin wrapper around the existing
//! [`crate::services::financial_data_service::FinancialDataService::fetch_news_sentiment`]
//! call. The wrapped service still owns the AV rate limiter, the
//! `news_cache` write-through, and the soft-skip-to-stale-cache fallback —
//! this adapter only exists to give downstream code a trait-shaped seam
//! so Phase 7 part B can land an `IbkrNewsProvider` and Phase 8 can
//! delete this adapter without touching call sites.
//!
//! Mirrors [`super::super::fundamentals_provider::alpha_vantage::AlphaVantageFundamentalsProvider`]:
//! the constructor takes an explicit `api_key_present` flag so a missing
//! AV key surfaces as a friendly typed error instead of the soft-skip
//! cache-only fallback the underlying service would otherwise use.
//! See Hard Invariant #5 — no silent mock / cache-only fallback on the
//! migration path.

use std::sync::Arc;

use async_trait::async_trait;

use crate::ibkr::types::news::NewsItem;
use crate::services::financial_data_service::FinancialDataService;

use super::{NewsError, NewsProvider};

/// Wraps the AV-backed [`FinancialDataService`] behind the trait. The
/// `inner` `Arc` is shared with the rest of the app — the same
/// `FinancialDataService` instance still serves the fundamentals path
/// through the [`super::super::fundamentals_provider`] composite.
pub struct AlphaVantageNewsProvider {
    inner: Arc<FinancialDataService>,
    api_key_present: bool,
}

impl AlphaVantageNewsProvider {
    /// Build a new adapter. The `api_key_present` flag short-circuits
    /// the upstream call when the operator has not configured an AV
    /// key, surfacing a friendly typed error instead of the
    /// cache-or-empty fallback the underlying news fetcher would
    /// otherwise use. See Hard Invariant #5 (no silent fallback on the
    /// migration path).
    pub fn new(inner: Arc<FinancialDataService>, api_key_present: bool) -> Self {
        Self {
            inner,
            api_key_present,
        }
    }
}

#[async_trait]
impl NewsProvider for AlphaVantageNewsProvider {
    async fn fetch(&self, symbol: &str, lookback_hours: u32) -> Result<Vec<NewsItem>, NewsError> {
        if !self.api_key_present {
            return Err(NewsError::Other(
                "Alpha Vantage API key not configured".to_string(),
            ));
        }
        match self
            .inner
            .fetch_news_sentiment(symbol, lookback_hours)
            .await
        {
            Ok(items) => Ok(items),
            Err(e) => Err(classify_av_error(e.as_ref())),
        }
    }
}

/// Crude string-matching error classifier. The wrapped
/// `FinancialDataService::fetch_news_sentiment` boxes its errors as
/// `Box<dyn Error>`, so the only interface we have for distinguishing
/// rate-limits from parse errors is the `Display` form. The message
/// strings come from `news.rs::classify_av_response` — `"Alpha Vantage
/// API error: ..."` for hard upstream errors, and the soft-skip path
/// turns into `Ok(cached-or-empty)` upstream so it never reaches this
/// classifier.
fn classify_av_error(err: &(dyn std::error::Error + Send + Sync)) -> NewsError {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("rate limit") || lower.contains("information") {
        NewsError::RateLimited { retry_after: None }
    } else if lower.contains("alpha vantage api error") {
        NewsError::ParseError(msg)
    } else {
        NewsError::Other(msg)
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    struct StringErr(String);

    #[test]
    fn classify_information_rate_limit_payload_to_rate_limited() {
        let err = StringErr(
            "Thank you for using Alpha Vantage! Our standard API rate limit \
             is 25 requests per day."
                .to_string(),
        );
        assert!(matches!(
            classify_av_error(&err),
            NewsError::RateLimited { .. }
        ));
    }

    #[test]
    fn classify_av_api_error_to_parse_error() {
        let err = StringErr("Alpha Vantage API error: invalid symbol".to_string());
        assert!(matches!(classify_av_error(&err), NewsError::ParseError(_)));
    }

    #[test]
    fn classify_unknown_to_other() {
        let err = StringErr("network unreachable".to_string());
        assert!(matches!(classify_av_error(&err), NewsError::Other(_)));
    }

    #[tokio::test]
    async fn empty_api_key_short_circuits_with_friendly_other_error() {
        let svc = Arc::new(FinancialDataService::new(String::new()));
        let provider = AlphaVantageNewsProvider::new(svc, false);
        let err = provider.fetch("AAPL", 24).await.expect_err("must error");
        let msg = err.to_string();
        assert!(
            msg.contains("Alpha Vantage API key not configured"),
            "expected friendly key-not-configured message, got: {msg}"
        );
        assert!(matches!(err, NewsError::Other(_)));
    }
}
