//! [`AlphaVantageFundamentalsProvider`] — the Phase 3 [`FundamentalsProvider`]
//! impl. Thin wrapper around the existing
//! [`crate::services::financial_data_service::FinancialDataService::fetch_fundamental_data`]
//! call. The wrapped service still owns the AV rate limiter, in-flight
//! coalescing map, file cache, and stale-cache fallback — this adapter
//! only exists to give downstream code a trait-shaped seam so the Phase 4
//! `CompositeFundamentalsProvider` can layer the manual store + AV
//! guardrails on top without changing call sites.

use std::sync::Arc;

use async_trait::async_trait;

use crate::ibkr::types::FundamentalData;
use crate::services::financial_data_service::FinancialDataService;

use super::{FundamentalsError, FundamentalsProvider};

/// Wraps the AV-backed [`FinancialDataService`] behind the trait. The
/// `inner` `Arc` is shared with the rest of the app — the same
/// `FinancialDataService` instance still serves the news path through
/// `fetch_news_sentiment`, which is intentionally NOT abstracted here
/// (Phase 7 introduces `NewsProvider`).
pub struct AlphaVantageFundamentalsProvider {
    inner: Arc<FinancialDataService>,
    api_key_present: bool,
}

impl AlphaVantageFundamentalsProvider {
    /// Build a new adapter. The `api_key_present` flag short-circuits
    /// the upstream call when the operator has not configured an AV key,
    /// surfacing a friendly typed error instead of the AV "Information"
    /// rate-limit response a key-less request elicits. See Hard
    /// Invariant #5 (no silent mock-data fallback).
    pub fn new(inner: Arc<FinancialDataService>, api_key_present: bool) -> Self {
        Self {
            inner,
            api_key_present,
        }
    }
}

#[async_trait]
impl FundamentalsProvider for AlphaVantageFundamentalsProvider {
    async fn fetch(&self, symbol: &str) -> Result<FundamentalData, FundamentalsError> {
        if !self.api_key_present {
            return Err(FundamentalsError::Other(
                "Alpha Vantage API key not configured".to_string(),
            ));
        }
        match self.inner.fetch_fundamental_data(symbol).await {
            Ok(data) => Ok(data),
            Err(e) => Err(classify_av_error(e.as_ref())),
        }
    }
}

/// Crude string-matching error classifier. The wrapped
/// `FinancialDataService` boxes its errors as `Box<dyn Error>`, so the
/// only interface we have for distinguishing rate-limits from "no data"
/// is the `Display` form. The message strings are stable inside this
/// crate (set by `fetch_av_function` and `do_fetch_fundamental_data`),
/// so this matching stays internal — no external caller relies on it.
fn classify_av_error(err: &(dyn std::error::Error + Send + Sync)) -> FundamentalsError {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("standard api rate limit")
        || lower.contains("rate limit")
        || lower.contains("information")
    {
        FundamentalsError::RateLimited { retry_after: None }
    } else if lower.contains("no historical financial data available") {
        FundamentalsError::NotFound(msg)
    } else if lower.contains("alpha vantage api error") {
        FundamentalsError::ParseError(msg)
    } else {
        FundamentalsError::Other(msg)
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
            "Alpha Vantage OVERVIEW: Thank you for using Alpha Vantage! \
             Our standard API rate limit is 25 requests per day."
                .to_string(),
        );
        assert!(matches!(
            classify_av_error(&err),
            FundamentalsError::RateLimited { .. }
        ));
    }

    #[test]
    fn classify_no_historical_data_to_not_found() {
        let err = StringErr(
            "No historical financial data available for ACME. This ticker may be too new..."
                .to_string(),
        );
        assert!(matches!(
            classify_av_error(&err),
            FundamentalsError::NotFound(_)
        ));
    }

    #[test]
    fn classify_av_api_error_to_parse_error() {
        let err = StringErr("Alpha Vantage API error: invalid symbol".to_string());
        assert!(matches!(
            classify_av_error(&err),
            FundamentalsError::ParseError(_)
        ));
    }

    #[test]
    fn classify_unknown_to_other() {
        let err = StringErr("network unreachable".to_string());
        assert!(matches!(
            classify_av_error(&err),
            FundamentalsError::Other(_)
        ));
    }

    #[tokio::test]
    async fn empty_api_key_short_circuits_with_friendly_other_error() {
        let svc = Arc::new(FinancialDataService::new(String::new()));
        let provider = AlphaVantageFundamentalsProvider::new(svc, false);
        let err = provider.fetch("AAPL").await.expect_err("must error");
        let msg = err.to_string();
        assert!(
            msg.contains("Alpha Vantage API key not configured"),
            "expected friendly key-not-configured message, got: {msg}"
        );
    }
}
