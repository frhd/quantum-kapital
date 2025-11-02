use super::auth::SheetsAuthenticator;
use super::service::GoogleSheetsService;
use super::types::*;
use crate::config::SettingsState;
use crate::ibkr::state::IbkrState;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// Global state for Google Sheets service
pub struct SheetsState {
    pub service: Arc<Mutex<Option<GoogleSheetsService>>>,
    pub config: Arc<Mutex<SheetsConfig>>,
}

impl SheetsState {
    pub fn new() -> Self {
        Self {
            service: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(SheetsConfig::default())),
        }
    }
}

/// Save Google OAuth2 credentials
#[tauri::command]
pub async fn save_google_credentials(
    credentials_json: String,
    _state: State<'_, SheetsState>,
) -> Result<String, String> {
    let authenticator = SheetsAuthenticator::new().map_err(|e| e.to_string())?;

    authenticator
        .save_credentials(&credentials_json)
        .await
        .map_err(|e| e.to_string())?;

    Ok("Credentials saved successfully".to_string())
}

/// Check if Google credentials are configured
#[tauri::command]
pub async fn check_google_credentials() -> Result<bool, String> {
    let authenticator = SheetsAuthenticator::new().map_err(|e| e.to_string())?;

    Ok(authenticator.has_credentials().await)
}

/// Authenticate with Google Sheets
#[tauri::command]
pub async fn google_sheets_authenticate(
    state: State<'_, SheetsState>,
) -> Result<AuthState, String> {
    let mut authenticator = SheetsAuthenticator::new().map_err(|e| e.to_string())?;

    // Perform authentication
    authenticator
        .authenticate()
        .await
        .map_err(|e| e.to_string())?;

    // Create and initialize the service
    let mut service = GoogleSheetsService::new(authenticator.clone());
    service.initialize().await.map_err(|e| e.to_string())?;

    // Store the service in state
    *state.service.lock().await = Some(service);

    // Return auth state
    Ok(authenticator.get_auth_state().await)
}

/// Disconnect from Google Sheets
#[tauri::command]
pub async fn google_sheets_disconnect(state: State<'_, SheetsState>) -> Result<String, String> {
    // Clear the service
    *state.service.lock().await = None;

    // Clear authentication
    let mut authenticator = SheetsAuthenticator::new().map_err(|e| e.to_string())?;

    authenticator
        .clear_auth()
        .await
        .map_err(|e| e.to_string())?;

    Ok("Disconnected from Google Sheets".to_string())
}

/// Get current Google Sheets authentication state
#[tauri::command]
pub async fn get_google_sheets_auth_state(
    _state: State<'_, SheetsState>,
) -> Result<AuthState, String> {
    let authenticator = SheetsAuthenticator::new().map_err(|e| e.to_string())?;

    Ok(authenticator.get_auth_state().await)
}

/// Create a new spreadsheet or get existing one
#[tauri::command]
pub async fn create_or_get_spreadsheet(
    name: String,
    sheets_state: State<'_, SheetsState>,
    settings_state: State<'_, SettingsState>,
) -> Result<String, String> {
    let service_lock = sheets_state.service.lock().await;
    let service = service_lock
        .as_ref()
        .ok_or("Not authenticated with Google Sheets")?;

    // Check if we already have a spreadsheet ID in persistent settings
    let settings = settings_state.config.read().await;
    if let Some(spreadsheet_id) = &settings.google_sheets.spreadsheet_id {
        return Ok(spreadsheet_id.clone());
    }
    drop(settings); // Release read lock

    // Create new spreadsheet
    let spreadsheet_id = service
        .create_spreadsheet(&name)
        .await
        .map_err(|e| e.to_string())?;

    // Save to persistent settings
    let mut settings = settings_state.config.write().await;
    settings.google_sheets.spreadsheet_id = Some(spreadsheet_id.clone());
    settings.google_sheets.spreadsheet_name = name.clone();
    settings
        .save()
        .await
        .map_err(|e| format!("Failed to save settings: {e}"))?;

    Ok(spreadsheet_id)
}

/// Export a single ticker's analysis to Google Sheets
#[tauri::command]
pub async fn export_ticker_to_sheets(
    ticker: String,
    analysis_data: TickerAnalysisData,
    sheets_state: State<'_, SheetsState>,
    settings_state: State<'_, SettingsState>,
) -> Result<ExportResult, String> {
    let service_lock = sheets_state.service.lock().await;
    let service = service_lock
        .as_ref()
        .ok_or("Not authenticated with Google Sheets")?;

    // Get spreadsheet from persistent settings
    let settings = settings_state.config.read().await;
    let spreadsheet_id = settings
        .google_sheets
        .spreadsheet_id
        .clone()
        .ok_or("No spreadsheet configured. Please create one first.")?;

    // Create ticker sheet
    service
        .create_ticker_sheet(&spreadsheet_id, &ticker)
        .await
        .map_err(|e| {
            // If sheet already exists, that's okay
            if e.to_string().contains("already exists") {
                // Continue with population
            } else {
                return e.to_string();
            }
            String::new()
        })
        .ok();

    // Populate ticker sheet
    service
        .populate_ticker_sheet(&spreadsheet_id, &ticker, &analysis_data)
        .await
        .map_err(|e| e.to_string())?;

    let url = service.get_spreadsheet_url(&spreadsheet_id);

    Ok(ExportResult {
        success: true,
        spreadsheet_id: spreadsheet_id.clone(),
        spreadsheet_url: url,
        sheets_created: vec![ticker.clone()],
        message: format!("Successfully exported {ticker} to Google Sheets"),
    })
}

/// Export all positions to Google Sheets
#[tauri::command]
pub async fn export_all_positions_to_sheets(
    ibkr_state: State<'_, IbkrState>,
    sheets_state: State<'_, SheetsState>,
    settings_state: State<'_, SettingsState>,
) -> Result<ExportResult, String> {
    let service_lock = sheets_state.service.lock().await;
    let service = service_lock
        .as_ref()
        .ok_or("Not authenticated with Google Sheets")?;

    // Get spreadsheet from persistent settings
    let settings = settings_state.config.read().await;
    let spreadsheet_id = settings
        .google_sheets
        .spreadsheet_id
        .clone()
        .ok_or("No spreadsheet configured. Please create one first.")?;

    // Get positions from IBKR state
    let positions = ibkr_state
        .client
        .get_positions()
        .await
        .map_err(|e| e.to_string())?;

    // Get tickers from positions
    let tickers: Vec<String> = positions.iter().map(|p| p.symbol.clone()).collect();

    // Create dashboard data
    let dashboard_data = DashboardData {
        total_positions: positions.len(),
        total_value: positions.iter().map(|p| p.market_value).sum(),
        analyzed_tickers: tickers.clone(),
        last_updated: chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
    };

    // Setup dashboard
    service
        .setup_dashboard(&spreadsheet_id, &dashboard_data)
        .await
        .map_err(|e| e.to_string())?;

    let url = service.get_spreadsheet_url(&spreadsheet_id);

    Ok(ExportResult {
        success: true,
        spreadsheet_id: spreadsheet_id.clone(),
        spreadsheet_url: url,
        sheets_created: vec!["Dashboard".to_string()],
        message: format!(
            "Successfully exported {} positions to Google Sheets",
            positions.len()
        ),
    })
}

/// Update dashboard with current portfolio data
#[tauri::command]
pub async fn update_dashboard(
    ibkr_state: State<'_, IbkrState>,
    sheets_state: State<'_, SheetsState>,
    settings_state: State<'_, SettingsState>,
) -> Result<String, String> {
    let service_lock = sheets_state.service.lock().await;
    let service = service_lock
        .as_ref()
        .ok_or("Not authenticated with Google Sheets")?;

    let settings = settings_state.config.read().await;
    let spreadsheet_id = settings
        .google_sheets
        .spreadsheet_id
        .clone()
        .ok_or("No spreadsheet configured")?;

    // Get positions
    let positions = ibkr_state
        .client
        .get_positions()
        .await
        .map_err(|e| e.to_string())?;

    let tickers: Vec<String> = positions.iter().map(|p| p.symbol.clone()).collect();

    let dashboard_data = DashboardData {
        total_positions: positions.len(),
        total_value: positions.iter().map(|p| p.market_value).sum(),
        analyzed_tickers: tickers,
        last_updated: chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
    };

    service
        .setup_dashboard(&spreadsheet_id, &dashboard_data)
        .await
        .map_err(|e| e.to_string())?;

    Ok("Dashboard updated successfully".to_string())
}

/// Get the current spreadsheet URL
#[tauri::command]
pub async fn get_spreadsheet_url(state: State<'_, SheetsState>) -> Result<String, String> {
    let service_lock = state.service.lock().await;
    let service = service_lock
        .as_ref()
        .ok_or("Not authenticated with Google Sheets")?;

    let config = state.config.lock().await;
    let spreadsheet_id = config
        .spreadsheet_id
        .clone()
        .ok_or("No spreadsheet configured")?;

    Ok(service.get_spreadsheet_url(&spreadsheet_id))
}
