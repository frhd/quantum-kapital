use crate::ibkr::types::{
    FundamentalData, ProjectionAssumptions, ProjectionResultsWithFundamentals, Quote,
    ScenarioProjectionsWithFundamentals,
};
use crate::services::cache_service::CacheService;
use crate::services::fundamentals_provider::{FundamentalsError, FundamentalsProvider};
use crate::services::projection_service::ProjectionService;
use crate::services::quote_service::QuoteService;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;
use tracing::info;

/// Stable error-string discriminants returned by the fundamentals
/// commands. Frontend hooks switch on these to render dedicated empty
/// states instead of a generic "fetch failed" banner.
///
/// Phase 3 removes the silent mock-data fallback that historically hid
/// upstream failures behind plausible-looking fake numbers (Hard
/// Invariant #5 — no silent fallback to mock data); a missing key,
/// rate-limit, or empty upstream now propagates as a typed signal the
/// UI can react to. The strings are part of the command contract.
fn map_fundamentals_error(err: &FundamentalsError) -> String {
    match err {
        FundamentalsError::RateLimited { .. } => "rate_limited".to_string(),
        FundamentalsError::NotConnected => "disconnected".to_string(),
        FundamentalsError::NotFound(_) => "no_data".to_string(),
        FundamentalsError::DailyBudgetExhausted { .. }
        | FundamentalsError::PerSymbolBudgetExhausted { .. } => "budget_exhausted".to_string(),
        FundamentalsError::ParseError(_) => "parse_error".to_string(),
        // Surface the message verbatim so the UI can show the
        // operator-curated "Alpha Vantage API key not configured" text
        // (and any other future Other variants) without losing detail.
        FundamentalsError::Other(msg) => msg.clone(),
    }
}

/// Fetch fundamentals via the trait-shaped provider. Phase 3 wires the
/// AV adapter directly; Phase 4 swaps in the composite (manual store →
/// AV cache → AV API) without changing this call site.
async fn fetch_fundamentals(
    provider: &Arc<dyn FundamentalsProvider>,
    symbol: &str,
) -> Result<FundamentalData, String> {
    info!("Fetching fundamentals for {symbol} via FundamentalsProvider");
    provider
        .fetch(symbol)
        .await
        .map_err(|e| map_fundamentals_error(&e))
}

/// Get fundamental data for a symbol via the configured provider.
/// Phase 3 removes the silent mock-data fallback; upstream failures
/// surface as typed error strings the frontend can switch on
/// (`rate_limited`, `no_data`, `disconnected`, `parse_error`,
/// `budget_exhausted`, or the `Other` payload verbatim).
#[tauri::command]
pub async fn ibkr_get_fundamental_data(
    fundamentals: State<'_, Arc<dyn FundamentalsProvider>>,
    symbol: String,
) -> Result<FundamentalData, String> {
    fetch_fundamentals(&fundamentals, &symbol).await
}

/// Generate financial projections based on fundamental data and assumptions
/// DEPRECATED: Use ibkr_generate_projection_results for better UI display
#[tauri::command]
pub async fn ibkr_generate_projections(
    fundamentals: State<'_, Arc<dyn FundamentalsProvider>>,
    symbol: String,
    assumptions: Option<ProjectionAssumptions>,
) -> Result<ScenarioProjectionsWithFundamentals, String> {
    let fundamentals = fetch_fundamentals(&fundamentals, &symbol).await?;
    let assumptions = assumptions.unwrap_or_default();
    let projections = ProjectionService::generate_projections(&fundamentals, &assumptions)
        .map_err(|e| e.to_string())?;
    Ok(ScenarioProjectionsWithFundamentals {
        fundamentals,
        projections,
    })
}

/// Generate projection results grouped by year (baseline + forward
/// projections). Returns the underlying fundamentals alongside so the
/// frontend can render projection inputs without a second fetch — this
/// is the dedup half of the AV-quota burn fix.
#[tauri::command]
pub async fn ibkr_generate_projection_results(
    fundamentals: State<'_, Arc<dyn FundamentalsProvider>>,
    symbol: String,
    assumptions: Option<ProjectionAssumptions>,
) -> Result<ProjectionResultsWithFundamentals, String> {
    let fundamentals = fetch_fundamentals(&fundamentals, &symbol).await?;
    let assumptions = assumptions.unwrap_or_default();
    let results = ProjectionService::generate_projection_results(&fundamentals, &assumptions)
        .map_err(|e| e.to_string())?;
    Ok(ProjectionResultsWithFundamentals {
        fundamentals,
        results,
    })
}

/// Get list of cached ticker symbols
/// Returns unique ticker symbols that have cached data
#[tauri::command]
pub async fn ibkr_get_cached_tickers() -> Result<Vec<String>, String> {
    // Initialize cache service with the same path used by FinancialDataService
    let cache = CacheService::new("cache/alphavantage").map_err(|e| e.to_string())?;

    // Get all valid cache keys
    let keys = cache.list_valid_keys().map_err(|e| e.to_string())?;

    // Extract unique ticker symbols from cache keys
    // Cache keys are like "AAPL_overview", "AAPL_income_statement", "AAPL_earnings"
    let mut tickers = HashSet::new();
    for key in keys {
        // Split by underscore and take the first part (the ticker symbol)
        if let Some(ticker) = key.split('_').next() {
            tickers.insert(ticker.to_string());
        }
    }

    // Convert to sorted vector
    let mut ticker_list: Vec<String> = tickers.into_iter().collect();
    ticker_list.sort();

    info!("Found {} cached tickers", ticker_list.len());
    Ok(ticker_list)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fundamentals_provider::test_support::FakeFundamentalsProvider;
    use std::time::Duration;

    /// Locks in the stable error-string discriminants the frontend
    /// switches on. Changing any of these is a contract break — update
    /// the consumer hooks in lockstep.
    #[test]
    fn map_fundamentals_error_emits_stable_discriminants() {
        assert_eq!(
            map_fundamentals_error(&FundamentalsError::RateLimited {
                retry_after: Some(Duration::from_secs(60))
            }),
            "rate_limited"
        );
        assert_eq!(
            map_fundamentals_error(&FundamentalsError::NotConnected),
            "disconnected"
        );
        assert_eq!(
            map_fundamentals_error(&FundamentalsError::NotFound("AAPL".into())),
            "no_data"
        );
        assert_eq!(
            map_fundamentals_error(&FundamentalsError::DailyBudgetExhausted { hit_count: 25 }),
            "budget_exhausted"
        );
        assert_eq!(
            map_fundamentals_error(&FundamentalsError::PerSymbolBudgetExhausted {
                symbol: "AAPL".into(),
            }),
            "budget_exhausted"
        );
        assert_eq!(
            map_fundamentals_error(&FundamentalsError::ParseError("bad json".into())),
            "parse_error"
        );
        // `Other` is passed through verbatim so the missing-API-key
        // banner can render the operator-curated text directly.
        assert_eq!(
            map_fundamentals_error(&FundamentalsError::Other(
                "Alpha Vantage API key not configured".into()
            )),
            "Alpha Vantage API key not configured"
        );
    }

    /// Replaces the pre-Phase-3 mock-data fallback test: an upstream
    /// failure must surface as a typed error string, NOT silently fill
    /// with `ProjectionService::generate_mock_fundamental_data` (Hard
    /// Invariant #5).
    #[tokio::test]
    async fn fetch_fundamentals_surfaces_provider_error_instead_of_mock_data() {
        let fake = FakeFundamentalsProvider::new();
        // No `insert(...)` — `FakeFundamentalsProvider` returns
        // `NotFound` for any unknown symbol, which `map_fundamentals_error`
        // collapses to the stable `"no_data"` discriminant.
        let provider: Arc<dyn FundamentalsProvider> = Arc::new(fake);
        let err = fetch_fundamentals(&provider, "AAPL")
            .await
            .expect_err("must surface upstream failure as typed error");
        assert_eq!(err, "no_data");
    }
}
