use crate::ibkr::types::{
    FundamentalData, ProjectionAssumptions, ProjectionResultsWithFundamentals, Quote,
    ScenarioProjectionsWithFundamentals,
};
use crate::services::cache_service::CacheService;
use crate::services::financial_data_service::FinancialDataService;
use crate::services::projection_service::ProjectionService;
use crate::services::quote_service::QuoteService;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;
use tracing::{info, warn};

/// Fetch fundamentals via the shared `FinancialDataService` (which
/// owns the AV rate limiter + in-flight coalescing). Falls back to
/// mock data on AV failure so the analysis UI keeps rendering when
/// the daily quota is exhausted. The mock-fallback escape hatch is
/// removed in Phase 3 of the IBKR migration; Phase 1 preserves it.
async fn get_fundamental_data_with_mock_fallback(
    financial: &FinancialDataService,
    symbol: &str,
) -> FundamentalData {
    info!(
        "Fetching real fundamental data for {} from Alpha Vantage API",
        symbol
    );
    match financial.fetch_fundamental_data(symbol).await {
        Ok(data) => {
            info!("Successfully fetched real fundamental data for {}", symbol);
            data
        }
        Err(e) => {
            warn!(
                "Failed to fetch real data for {}: {}. Falling back to mock data.",
                symbol, e
            );
            ProjectionService::generate_mock_fundamental_data(symbol)
        }
    }
}

/// Get fundamental data for a symbol
/// Fetches real data from Alpha Vantage API if available,
/// otherwise falls back to mock data for testing.
#[tauri::command]
pub async fn ibkr_get_fundamental_data(
    financial: State<'_, Arc<FinancialDataService>>,
    symbol: String,
) -> Result<FundamentalData, String> {
    Ok(get_fundamental_data_with_mock_fallback(&financial, &symbol).await)
}

/// Generate financial projections based on fundamental data and assumptions
/// DEPRECATED: Use ibkr_generate_projection_results for better UI display
#[tauri::command]
pub async fn ibkr_generate_projections(
    financial: State<'_, Arc<FinancialDataService>>,
    symbol: String,
    assumptions: Option<ProjectionAssumptions>,
) -> Result<ScenarioProjectionsWithFundamentals, String> {
    let fundamentals = get_fundamental_data_with_mock_fallback(&financial, &symbol).await;
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
    financial: State<'_, Arc<FinancialDataService>>,
    symbol: String,
    assumptions: Option<ProjectionAssumptions>,
) -> Result<ProjectionResultsWithFundamentals, String> {
    let fundamentals = get_fundamental_data_with_mock_fallback(&financial, &symbol).await;
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
