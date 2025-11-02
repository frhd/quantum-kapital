use crate::ibkr::types::{FundamentalData, ProjectionAssumptions, ScenarioProjections};
use crate::services::projection_service::ProjectionService;
use tauri::State;

use crate::ibkr::state::IbkrState;

/// Get fundamental data for a symbol
/// For now, returns mock data. Will be replaced with real IBKR API call.
#[tauri::command]
pub async fn ibkr_get_fundamental_data(
    _state: State<'_, IbkrState>,
    symbol: String,
) -> Result<FundamentalData, String> {
    // TODO: Replace with real IBKR API call when implemented
    // For now, return mock data for testing
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
    ProjectionService::generate_projections(&fundamental, &assumptions)
        .map_err(|e| e.to_string())
}
