use crate::ibkr::types::{FundamentalData, ProjectionAssumptions, ScenarioProjections};
use crate::services::financial_data_service::FinancialDataService;
use crate::services::projection_service::ProjectionService;
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
