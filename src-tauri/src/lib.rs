mod config;
mod events;
mod ibkr;
mod middleware;
mod services;
mod utils;

use config::{AppConfig, SettingsState};
use ibkr::IbkrState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load environment variables from .env file
    dotenv::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Load configuration from disk or use defaults
            let config = AppConfig::load_sync().unwrap_or_default();

            // Initialize settings state
            let settings_state = SettingsState::new(config.clone());

            // Initialize IBKR state with configuration
            let ibkr_state = IbkrState::new(config.ibkr.clone().into());

            // Set app handle for event emitter
            let app_handle = app.handle().clone();
            let state_clone = ibkr_state.clone();
            tauri::async_runtime::spawn(async move {
                state_clone.event_emitter.set_app_handle(app_handle).await;
            });

            app.manage(settings_state);
            app.manage(ibkr_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ibkr::commands::ibkr_connect,
            ibkr::commands::ibkr_disconnect,
            ibkr::commands::ibkr_get_connection_status,
            ibkr::commands::ibkr_get_accounts,
            ibkr::commands::ibkr_get_account_summary,
            ibkr::commands::ibkr_get_positions,
            ibkr::commands::ibkr_subscribe_market_data,
            ibkr::commands::ibkr_place_order,
            ibkr::commands::ibkr_get_fundamental_data,
            ibkr::commands::ibkr_generate_projections,
            ibkr::commands::ibkr_generate_projection_results,
            ibkr::commands::ibkr_get_cached_tickers,
            config::commands::get_settings,
            config::commands::update_settings,
            config::commands::get_settings_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
