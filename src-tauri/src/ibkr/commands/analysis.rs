use crate::ibkr::types::{
    FundamentalData, ProjectionAssumptions, ProjectionResults, Quote, ScenarioProjections,
};
use crate::services::cache_service::CacheService;
use crate::services::financial_data_service::FinancialDataService;
use crate::services::projection_service::ProjectionService;
use crate::services::quote_service::QuoteService;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;
use tracing::{info, warn};

use crate::ibkr::state::IbkrState;

/// Get fundamental data for a symbol
/// Fetches real data from Alpha Vantage API if available,
/// otherwise falls back to mock data for testing.
#[tauri::command]
pub async fn ibkr_get_fundamental_data(
    _state: State<'_, IbkrState>,
    symbol: String,
) -> Result<FundamentalData, String> {
    // Try to get API key from environment
    let api_key = std::env::var("ALPHA_VANTAGE_API_KEY");

    if let Ok(key) = api_key {
        // Try to fetch real data from Alpha Vantage
        info!(
            "Fetching real fundamental data for {} from Alpha Vantage API",
            symbol
        );
        let service = FinancialDataService::new(key);

        match service.fetch_fundamental_data(&symbol).await {
            Ok(data) => {
                info!("Successfully fetched real fundamental data for {}", symbol);
                return Ok(data);
            }
            Err(e) => {
                warn!(
                    "Failed to fetch real data for {}: {}. Falling back to mock data.",
                    symbol, e
                );
            }
        }
    } else {
        info!(
            "ALPHA_VANTAGE_API_KEY not set. Using mock data for {}. Set ALPHA_VANTAGE_API_KEY environment variable to fetch real data.",
            symbol
        );
    }

    // Fallback to mock data
    Ok(ProjectionService::generate_mock_fundamental_data(&symbol))
}

/// Generate financial projections based on fundamental data and assumptions
/// DEPRECATED: Use ibkr_generate_projection_results for better UI display
#[tauri::command]
pub async fn ibkr_generate_projections(
    state: State<'_, IbkrState>,
    symbol: String,
    assumptions: Option<ProjectionAssumptions>,
) -> Result<ScenarioProjections, String> {
    // Get fundamental data
    let fundamental = ibkr_get_fundamental_data(state, symbol)
        .await
        .map_err(|e| format!("Failed to get fundamental data: {e}"))?;

    // Use provided assumptions or default
    let assumptions = assumptions.unwrap_or_default();

    // Generate projections
    ProjectionService::generate_projections(&fundamental, &assumptions).map_err(|e| e.to_string())
}

/// Generate projection results grouped by year (baseline + forward projections)
/// This format is preferred for UI display as it shows bear/base/bull side-by-side for each year
#[tauri::command]
pub async fn ibkr_generate_projection_results(
    state: State<'_, IbkrState>,
    symbol: String,
    assumptions: Option<ProjectionAssumptions>,
) -> Result<ProjectionResults, String> {
    // Get fundamental data
    let fundamental = ibkr_get_fundamental_data(state, symbol)
        .await
        .map_err(|e| format!("Failed to get fundamental data: {e}"))?;

    // Use provided assumptions or default
    let assumptions = assumptions.unwrap_or_default();

    // Generate projection results
    ProjectionService::generate_projection_results(&fundamental, &assumptions)
        .map_err(|e| e.to_string())
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
#[allow(dead_code)] // registered in lib.rs in Task 9
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
