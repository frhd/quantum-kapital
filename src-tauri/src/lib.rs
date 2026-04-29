mod config;
mod events;
mod ibkr;
mod middleware;
mod services;
mod storage;
mod strategies;
mod utils;

use std::sync::Arc;

use config::{AppConfig, SettingsState};
use ibkr::IbkrState;
use middleware::HistoricalRateLimiter;
use services::historical_data_service::{HistoricalDataFetcher, HistoricalDataService};
use storage::Db;
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

            // Open SQLite tracker database in app local data dir. The DB
            // is shared with `IbkrState` (for the tracker service) and
            // with the historical-bars service constructed below.
            let db_dir = app
                .path()
                .app_local_data_dir()
                .map_err(|e| format!("resolve app_local_data_dir: {e}"))?;
            std::fs::create_dir_all(&db_dir)
                .map_err(|e| format!("create app data dir {db_dir:?}: {e}"))?;
            let db_path = db_dir.join("tracker.sqlite");
            let db =
                Db::open(&db_path).map_err(|e| format!("open tracker db at {db_path:?}: {e}"))?;
            let db = Arc::new(db);

            // Initialize IBKR state with configuration + shared DB.
            let ibkr_state = IbkrState::new(config.ibkr.clone().into(), Arc::clone(&db));

            // Set app handle for event emitter
            let app_handle = app.handle().clone();
            let state_clone = ibkr_state.clone();
            tauri::async_runtime::spawn(async move {
                state_clone.event_emitter.set_app_handle(app_handle).await;
            });

            // Construct the historical-bars service. The IBKR client is
            // shared with `IbkrState`; the rate limiter is per-service so
            // different feature areas can carry their own budgets later.
            let hist_rate_limit = Arc::new(HistoricalRateLimiter::new(6));
            let fetcher: Arc<dyn HistoricalDataFetcher> =
                Arc::clone(&ibkr_state.client) as Arc<dyn HistoricalDataFetcher>;
            let hist_service = Arc::new(HistoricalDataService::new(
                Arc::clone(&db),
                fetcher,
                hist_rate_limit,
            ));

            app.manage(settings_state);
            app.manage(ibkr_state);
            app.manage(db);
            app.manage(hist_service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ibkr::commands::ibkr_connect,
            ibkr::commands::ibkr_disconnect,
            ibkr::commands::ibkr_get_connection_status,
            ibkr::commands::ibkr_get_accounts,
            ibkr::commands::ibkr_get_account_summary,
            ibkr::commands::ibkr_get_positions,
            ibkr::commands::ibkr_start_daily_pnl,
            ibkr::commands::ibkr_stop_daily_pnl,
            ibkr::commands::ibkr_subscribe_market_data,
            ibkr::commands::ibkr_place_order,
            ibkr::commands::ibkr_get_fundamental_data,
            ibkr::commands::ibkr_generate_projections,
            ibkr::commands::ibkr_generate_projection_results,
            ibkr::commands::ibkr_get_cached_tickers,
            ibkr::commands::ibkr_start_scanner,
            ibkr::commands::ibkr_stop_scanner,
            ibkr::commands::tracker_fetch_bars,
            ibkr::commands::tracker_get_news,
            ibkr::commands::tracker_add,
            ibkr::commands::tracker_remove,
            ibkr::commands::tracker_list,
            ibkr::commands::tracker_get,
            ibkr::commands::tracker_set_tags,
            ibkr::commands::tracker_set_status,
            config::commands::get_settings,
            config::commands::update_settings,
            config::commands::get_settings_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
