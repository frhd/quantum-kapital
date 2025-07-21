mod ibkr;

use tauri::Manager;
use ibkr::{IbkrState, types::ConnectionConfig};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Initialize IBKR state with default configuration
            let ibkr_state = IbkrState::new(ConnectionConfig::default());
            app.manage(ibkr_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            ibkr::commands::ibkr_connect,
            ibkr::commands::ibkr_disconnect,
            ibkr::commands::ibkr_get_connection_status,
            ibkr::commands::ibkr_get_accounts,
            ibkr::commands::ibkr_get_account_summary,
            ibkr::commands::ibkr_get_positions,
            ibkr::commands::ibkr_subscribe_market_data,
            ibkr::commands::ibkr_place_order,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
