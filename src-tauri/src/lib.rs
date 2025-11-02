mod config;
mod events;
mod ibkr;
mod middleware;
mod services;
mod utils;

use config::AppConfig;
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
            // Load configuration
            let config = AppConfig::default(); // In production, would load from file

            // Initialize IBKR state with configuration
            let ibkr_state = IbkrState::new(config.ibkr.clone().into());

            // Set app handle for event emitter
            let app_handle = app.handle().clone();
            let state_clone = ibkr_state.clone();
            tauri::async_runtime::spawn(async move {
                state_clone.event_emitter.set_app_handle(app_handle).await;
            });

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
