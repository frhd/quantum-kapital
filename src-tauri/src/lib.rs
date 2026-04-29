mod config;
mod events;
mod ibkr;
mod middleware;
mod services;
mod storage;
mod strategies;
mod utils;

use std::sync::Arc;

use std::time::Duration;

use config::{AppConfig, SettingsState};
use ibkr::IbkrState;
use middleware::HistoricalRateLimiter;
use services::daily_ranker::DailyRanker;
use services::decay_watcher::{DecayWatcher, LlmDecayWatcher};
use services::eod_scheduler::EodScheduler;
use services::financial_data_service::FinancialDataService;
use services::historical_data_service::{HistoricalDataFetcher, HistoricalDataService};
use services::intraday_scheduler::IntradayScheduler;
use services::llm_service::LlmService;
use services::news_interpreter::NewsInterpreter;
use services::thesis_generator::ThesisGenerator;
use services::tracker_runner::{BarsFetcher, NewsFetcher, TrackerRunner};
use storage::Db;
use strategies::registry_from_config;
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

            // Phase 16: LLM service. API key read from config (which sources
            // from the ANTHROPIC_API_KEY env var via Default for ApiConfig).
            let anthropic_api_key = config.api.anthropic_api_key.clone().unwrap_or_default();
            let llm_service = Arc::new(LlmService::new(
                anthropic_api_key,
                Arc::clone(&db),
                config.api.daily_llm_budget_usd,
            ));

            // Initialize IBKR state with configuration + shared DB.
            let ibkr_state = IbkrState::new(
                config.ibkr.clone().into(),
                Arc::clone(&db),
                Arc::clone(&llm_service),
            );

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

            // Phase 10: shared FinancialDataService instance (the news
            // half is best-effort and falls back to cached/empty when
            // the API key is missing or rate-limited). The tracker
            // runner lifts bars + news + the detector registry into a
            // single command-callable surface.
            let api_key = std::env::var("ALPHA_VANTAGE_API_KEY").unwrap_or_default();
            // Phase 19: news interpreter runs after each successful AV
            // news fetch and lands a structured NewsVerdict in
            // news_cache.news_verdict_json. Best-effort — interpreter
            // failures never propagate.
            let news_interpreter = Arc::new(NewsInterpreter::new(
                Arc::clone(&llm_service),
                Arc::clone(&db),
            ));
            let financial_service = Arc::new(
                FinancialDataService::new(api_key)
                    .with_db(Arc::clone(&db))
                    .with_news_interpreter(Arc::clone(&news_interpreter)),
            );

            let bars: Arc<dyn BarsFetcher> = Arc::clone(&hist_service) as Arc<dyn BarsFetcher>;
            let decay_bars: Arc<dyn BarsFetcher> =
                Arc::clone(&hist_service) as Arc<dyn BarsFetcher>;
            let news: Arc<dyn NewsFetcher> = Arc::clone(&financial_service) as Arc<dyn NewsFetcher>;

            // Phase 17: thesis generator runs after each persisted setup
            // and re-emits `SetupDetected` with the populated thesis.
            let thesis_generator = Arc::new(ThesisGenerator::new(
                Arc::clone(&llm_service),
                Arc::clone(&ibkr_state.tracker),
                Arc::clone(&ibkr_state.event_emitter),
            ));

            let tracker_runner = Arc::new(
                TrackerRunner::new(
                    Arc::clone(&db),
                    Arc::clone(&ibkr_state.tracker),
                    Arc::clone(&ibkr_state.state_machine),
                    Arc::clone(&ibkr_state.event_emitter),
                    bars,
                    news,
                    Arc::new(registry_from_config(&config.detectors)),
                )
                .with_thesis_generator(Arc::clone(&thesis_generator)),
            );

            // Phase 20: daily ranker — picks the LLM-ranked top-5 from
            // today's setups after the EOD sweep, persists to
            // `morning_packs`, and emits `MorningPackReady`.
            let daily_ranker = Arc::new(DailyRanker::new(
                Arc::clone(&llm_service),
                Arc::clone(&ibkr_state.tracker),
                Arc::clone(&db),
                Arc::clone(&ibkr_state.event_emitter),
            ));

            // Phase 13: EOD scheduler. The handle is held on `IbkrState`
            // and started/stopped via the `tracker_start_scheduler` /
            // `tracker_stop_scheduler` commands — auto-start is
            // intentionally off by default (the user opts in from the UI
            // once Phase 15's frontend listeners land).
            let eod_scheduler = Arc::new(
                EodScheduler::new(
                    Arc::clone(&tracker_runner),
                    Arc::clone(&ibkr_state.state_machine),
                    Arc::clone(&ibkr_state.event_emitter),
                )
                .with_daily_ranker(Arc::clone(&daily_ranker)),
            );

            // Phase 14: intraday scheduler. Same start/stop command pair
            // as the EOD scheduler. Phase 18 swapped the stub for a real
            // Anthropic-backed `LlmDecayWatcher` (Haiku 4.5).
            let decay_watcher: Arc<dyn DecayWatcher> =
                Arc::new(LlmDecayWatcher::new(Arc::clone(&llm_service), decay_bars));
            let intraday_scheduler = Arc::new(IntradayScheduler::new(
                Arc::clone(&tracker_runner),
                Arc::clone(&ibkr_state.state_machine),
                Arc::clone(&ibkr_state.tracker),
                decay_watcher,
                Duration::from_secs(config.tracker.intraday_tick_interval_secs),
            ));

            app.manage(settings_state);
            app.manage(ibkr_state);
            app.manage(db);
            app.manage(hist_service);
            app.manage(financial_service);
            app.manage(tracker_runner);
            app.manage(eod_scheduler);
            app.manage(intraday_scheduler);
            app.manage(llm_service);
            app.manage(thesis_generator);
            app.manage(news_interpreter);
            app.manage(daily_ranker);
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
            ibkr::commands::tracker_run_now,
            ibkr::commands::tracker_get_setups,
            ibkr::commands::tracker_start_scheduler,
            ibkr::commands::tracker_stop_scheduler,
            ibkr::commands::tracker_get_morning_pack,
            ibkr::commands::tracker_list_alerts,
            ibkr::commands::tracker_mark_alerts_seen,
            #[cfg(debug_assertions)]
            ibkr::commands::tracker_llm_smoke_test,
            config::commands::get_settings,
            config::commands::update_settings,
            config::commands::get_settings_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
