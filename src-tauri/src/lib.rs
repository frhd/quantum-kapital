mod config;
mod events;
mod ibkr;
pub mod mcp;
mod middleware;
mod services;
mod storage;
mod strategies;
mod utils;

// Targeted re-exports for the `flex_backfill` binary in `bin/flex_backfill.rs`.
// Keeps `ibkr` / `services` / `storage` private to the lib while letting the
// one-shot importer share the live ingestor's types and store.
pub use ibkr::types::{ExecutionSide, IbkrExecution};
pub use services::executions::ExecutionsStore;
pub use storage::Db;

use std::sync::Arc;

use std::time::Duration;

use config::{settings::LlmBackendKind, AppConfig, SettingsState};
use ibkr::IbkrState;
use middleware::{AlphaVantageRateLimiter, HistoricalRateLimiter, IbkrNewsRateLimiter};
use services::auto_scanner::{AutoScannerScheduler, AutoScannerService, MarketScanner};
use services::daily_ranker::DailyRanker;
use services::decay_watcher::{DecayWatcher, LlmDecayWatcher};
use services::eod_scheduler::EodScheduler;
use services::executions::{ExecutionsIngestor, LiveExecutionsFetcher};
use services::financial_data_service::FinancialDataService;
use services::fundamentals_provider::alpha_vantage::AlphaVantageFundamentalsProvider;
use services::fundamentals_provider::av_call_ledger::AvCallLedger;
use services::fundamentals_provider::composite::{AvGuard, CompositeFundamentalsProvider};
use services::fundamentals_provider::manual::ManualFundamentalsProvider;
use services::fundamentals_provider::FundamentalsProvider;
use services::historical_data_service::{HistoricalDataFetcher, HistoricalDataService};
use services::intraday_scheduler::IntradayScheduler;
use services::llm_service::{
    backend::LlmBackend, ApiBackend, ClaudeCliBackend, LlmService, ReqwestAnthropicHttp,
};
use services::manual_fundamentals_store::ManualFundamentalsStore;
use services::news_interpreter::NewsInterpreter;
use services::news_provider::ibkr::client::IbkrNewsClient;
use services::news_provider::ibkr::IbkrNewsProvider;
use services::news_provider::NewsProvider;
use services::risk_engine::{EquityFetcher, EquitySnapshotService, RiskEngine};
use services::social_sentiment::apewisdom::ApewisdomProvider;
use services::social_sentiment::provider::{ReqwestHttpFetcher, SentimentProvider};
use services::social_sentiment::reddit::RedditWsbProvider;
use services::social_sentiment::stocktwits::StocktwitsProvider;
use services::social_sentiment::SocialSentimentService;
use services::social_sentiment_scheduler::SocialSentimentScheduler;
use services::thesis_generator::ThesisGenerator;
use services::ticker_primer::TickerPrimerService;
use services::tracker_runner::{BarsFetcher, TrackerRunner};
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
        .plugin(tauri_plugin_dialog::init())
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

            // Phase 16 + LLM-backend split: select the transport based
            // on `QK_LLM_BACKEND` (sourced into `ApiConfig.llm_backend`).
            // Default `Anthropic` posts to api.anthropic.com using the
            // configured api key. `ClaudeCli` runs `claude -p` under
            // the user's Claude Code subscription. Probe runs once at
            // startup and refuses to construct the service if the
            // binary is missing — no silent fallback to the API path.
            let anthropic_api_key = config.api.anthropic_api_key.clone().unwrap_or_default();
            let llm_backend: Arc<dyn LlmBackend> = match config.api.llm_backend {
                LlmBackendKind::Anthropic => {
                    let http = Arc::new(ReqwestAnthropicHttp::new());
                    Arc::new(ApiBackend::new(http, anthropic_api_key.clone()))
                }
                LlmBackendKind::ClaudeCli => {
                    let binary =
                        std::env::var("QK_CLAUDE_BINARY").unwrap_or_else(|_| "claude".to_string());
                    let bin_path = std::path::PathBuf::from(&binary);
                    let version = ClaudeCliBackend::probe_version(&bin_path).map_err(|e| {
                        format!("QK_LLM_BACKEND=claude_cli but probe failed for {binary}: {e}")
                    })?;
                    Arc::new(ClaudeCliBackend::new(
                        bin_path,
                        version,
                        crate::services::llm_service::cli_backend::DEFAULT_TIMEOUT,
                    ))
                }
            };
            tracing::info!(
                backend = llm_backend.kind(),
                version = llm_backend.version().unwrap_or(""),
                "llm_service: backend ready"
            );
            let llm_service = Arc::new(LlmService::new_with_backend(
                llm_backend,
                Arc::clone(&db),
                config.api.daily_llm_budget_usd,
            ));

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

            // Phase 21: live-quote service. Wraps IbkrClient through
            // the QuoteFetcher seam so the command + tests share the
            // same interface, and the snapshot is never cached.
            let quote_fetcher: Arc<dyn crate::services::quote_service::QuoteFetcher> =
                Arc::clone(&ibkr_state.client)
                    as Arc<dyn crate::services::quote_service::QuoteFetcher>;
            let quote_service = Arc::new(crate::services::quote_service::QuoteService::new(
                quote_fetcher,
            ));

            // Shared `FinancialDataService` instance — after the Phase 8
            // AV-news strip-out it serves only the fundamentals
            // fallback inside `CompositeFundamentalsProvider`. News is
            // owned by `IbkrNewsProvider` (constructed below).
            let api_key = std::env::var("ALPHA_VANTAGE_API_KEY").unwrap_or_default();
            let api_key_present = !api_key.trim().is_empty();
            // Phase 19: news interpreter runs after each successful
            // news fetch and lands a structured NewsVerdict in
            // news_cache.news_verdict_json. Wired into the IBKR news
            // provider below so a fresh fetch invalidates any stale
            // verdict and re-runs the interpreter best-effort.
            let news_interpreter = Arc::new(NewsInterpreter::new(
                Arc::clone(&llm_service),
                Arc::clone(&db),
            ));
            // Phase 1 (AV migration): the AV free tier permits 1 req/sec
            // across the whole account. Without this serialiser, three
            // parallel fundamentals fetches burn the per-second cap in
            // a single tick and the next call returns "Information"
            // rate-limit payloads.
            let av_rate_limiter = Arc::new(AlphaVantageRateLimiter::per_second());
            let financial_service = Arc::new(
                FinancialDataService::new(api_key).with_rate_limiter(Arc::clone(&av_rate_limiter)),
            );

            // Phase 4 (AV migration): the production fundamentals path
            // is the composite (manual store → AV adapter). The AV
            // branch retains the Phase-1 file cache, in-flight
            // coalescing, and stale-cache fallback because the
            // FinancialDataService it wraps is unchanged. Hard
            // Invariant #8: the composite checks the manual store
            // first; manual writes invalidate the AV file cache for
            // the same symbol via `set_fundamentals`.
            let manual_fundamentals_store = Arc::new(ManualFundamentalsStore::new(Arc::clone(&db)));
            let av_fundamentals_provider: Arc<dyn FundamentalsProvider> =
                Arc::new(AlphaVantageFundamentalsProvider::new(
                    Arc::clone(&financial_service),
                    api_key_present,
                ));
            let manual_fundamentals_provider = Arc::new(ManualFundamentalsProvider::new(
                Arc::clone(&manual_fundamentals_store),
            ));

            // Phase 5: AV-side guardrails. The ledger gates the AV
            // branch of the composite (per-symbol cap + daily soft/hard
            // cap, persisted to SQLite so a process restart cannot
            // silently double-up against the AV free-tier quota). The
            // separate CacheService handle is the AvGuard's cache-fresh
            // probe + stale fallback reader; it points at the same
            // "cache/alphavantage" directory the FinancialDataService
            // writes to, so the two views never diverge.
            let av_call_ledger = Arc::new(AvCallLedger::new(Arc::clone(&db)));
            let av_cache_for_guard = Arc::new(
                services::cache_service::CacheService::new("cache/alphavantage")
                    .expect("AV cache directory init"),
            );
            let av_guard = Arc::new(AvGuard::new(
                Arc::clone(&av_call_ledger),
                Arc::clone(&av_cache_for_guard),
            ));
            let fundamentals_provider: Arc<dyn FundamentalsProvider> = Arc::new(
                CompositeFundamentalsProvider::new(
                    Arc::clone(&manual_fundamentals_provider),
                    Arc::clone(&av_fundamentals_provider),
                )
                .with_av_guard(Arc::clone(&av_guard)),
            );

            let bars: Arc<dyn BarsFetcher> = Arc::clone(&hist_service) as Arc<dyn BarsFetcher>;
            let decay_bars: Arc<dyn BarsFetcher> =
                Arc::clone(&hist_service) as Arc<dyn BarsFetcher>;

            // Phase 8 deletion: AV news is gone — the production news
            // path is exclusively `IbkrNewsProvider`. The same
            // `IbkrClient` connection that serves bars / scanner /
            // quote impls `IbkrNewsClient` (see `ibkr/client/news.rs`).
            // 30 calls/min mirrors the Phase 6 spike pacing.
            // `with_news_cache` write-throughs every successful fetch
            // to the `news_cache` SQLite table so `NewsInterpreter` and
            // the MCP `get_news` tool keep seeing fresh rows;
            // `with_news_interpreter` runs the verdict pass best-effort
            // after each write.
            let news_client: Arc<dyn IbkrNewsClient> =
                Arc::clone(&ibkr_state.client) as Arc<dyn IbkrNewsClient>;
            let news_rate_limiter = Arc::new(IbkrNewsRateLimiter::new(30));
            let news_provider: Arc<dyn NewsProvider> = Arc::new(
                IbkrNewsProvider::new(news_client, news_rate_limiter)
                    .with_news_cache(Arc::clone(&db))
                    .with_news_interpreter(Arc::clone(&news_interpreter)),
            );

            // Phase 17: thesis generator runs after each persisted setup
            // and re-emits `SetupDetected` with the populated thesis.
            let thesis_generator = Arc::new(ThesisGenerator::new(
                Arc::clone(&llm_service),
                Arc::clone(&ibkr_state.tracker),
                Arc::clone(&ibkr_state.event_emitter),
            ));

            // Ticker-intake Phase 1: post-add fundamentals → projection
            // → news primer. Fire-and-forget from both add callers
            // (`add_ticker` MCP tool + `tracker_add` Tauri command).
            // Reuses the existing AV cache directory so projection JSON
            // sits next to fundamentals JSON; the cache key is
            // `{SYMBOL}_projection` which is disjoint from the AV
            // `{SYMBOL}_overview` / `_income_statement` / `_earnings`
            // keys.
            let ticker_primer = Arc::new(TickerPrimerService::new(
                Arc::clone(&ibkr_state.tracker),
                Arc::clone(&fundamentals_provider),
                Arc::clone(&news_provider),
                Arc::clone(&av_cache_for_guard),
                Arc::clone(&ibkr_state.event_emitter),
            ));

            // Quant-decisions Phase 1 — risk engine. Sizes every
            // detector hit against a T-1 NLV snapshot pulled from
            // IBKR. Knobs come from `config.risk_engine` and stay
            // editable at runtime via `risk_set_config`. The
            // `IbkrClient` impls both `EquityFetcher` and
            // `AccountSource`, so the production wiring is just two
            // upcasts.
            let equity_fetcher: Arc<dyn EquityFetcher> =
                Arc::clone(&ibkr_state.client) as Arc<dyn EquityFetcher>;
            let equity_snapshot_svc =
                Arc::new(EquitySnapshotService::new(Arc::clone(&db), equity_fetcher));
            let account_source: Arc<dyn services::risk_engine::AccountSource> =
                Arc::clone(&ibkr_state.client) as Arc<dyn services::risk_engine::AccountSource>;
            let risk_engine = Arc::new(RiskEngine::new(
                Arc::clone(&equity_snapshot_svc),
                account_source,
                config.risk_engine.clone(),
            ));

            let tracker_runner = Arc::new(
                TrackerRunner::new(
                    Arc::clone(&db),
                    Arc::clone(&ibkr_state.tracker),
                    Arc::clone(&ibkr_state.state_machine),
                    Arc::clone(&ibkr_state.event_emitter),
                    bars,
                    Arc::clone(&news_provider),
                    Arc::new(registry_from_config(&config.detectors)),
                )
                .with_thesis_generator(Arc::clone(&thesis_generator))
                .with_data_tier(Arc::clone(&ibkr_state.data_tier))
                .with_risk_engine(Arc::clone(&risk_engine)),
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

            // First automation step: scheduled IBKR scanner that
            // promotes top-ranked rows into the watchlist with
            // `source = auto_scanner`. Ships dark — auto-start only
            // fires when the user has flipped `auto_scanner.enabled`
            // to true in settings. Otherwise the scheduler stays
            // available for manual `auto_scanner_start`.
            let market_scanner: Arc<dyn MarketScanner> =
                Arc::clone(&ibkr_state.client) as Arc<dyn MarketScanner>;
            let candidate_universe = Arc::new(
                services::candidate_universe::CandidateUniverseService::new(Arc::clone(&db)),
            );
            let candidate_promoter =
                Arc::new(services::candidate_promoter::CandidatePromoter::new(
                    Arc::clone(&candidate_universe),
                    Arc::clone(&ibkr_state.tracker),
                    config.auto_scanner.auto_promote_threshold,
                ));
            let auto_scanner_service = Arc::new(AutoScannerService::new(
                market_scanner,
                Arc::clone(&ibkr_state.tracker),
                Arc::clone(&candidate_promoter),
                Arc::clone(&db),
                config.auto_scanner.clone(),
            ));
            let auto_scanner_scheduler = Arc::new(AutoScannerScheduler::new(
                Arc::clone(&auto_scanner_service),
                Duration::from_secs(60),
            ));
            if config.auto_scanner.enabled {
                let scheduler = Arc::clone(&auto_scanner_scheduler);
                let state_for_spawn = ibkr_state.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = state_for_spawn.start_auto_scanner(scheduler).await {
                        tracing::warn!("auto-scanner auto-start failed: {e}");
                    }
                });
            }

            // Phase 3: social-sentiment ingestion. Provider list is
            // built from `social_sentiment` settings — flipping a
            // source off in settings.json drops it from the fan-out.
            // The scheduler ships dark; the user opts in via
            // `social_sentiment.enabled = true` and the `social_*`
            // Tauri commands (or by editing settings.json directly).
            let sentiment_http: Arc<dyn services::social_sentiment::provider::HttpFetcher> =
                Arc::new(ReqwestHttpFetcher::default());
            let mut sentiment_providers: Vec<Arc<dyn SentimentProvider>> = Vec::new();
            if config.social_sentiment.source_apewisdom_enabled {
                sentiment_providers.push(Arc::new(ApewisdomProvider::new(Arc::clone(
                    &sentiment_http,
                ))));
            }
            if config.social_sentiment.source_stocktwits_enabled {
                sentiment_providers.push(Arc::new(StocktwitsProvider::new(Arc::clone(
                    &sentiment_http,
                ))));
            }
            if config.social_sentiment.source_reddit_enabled {
                sentiment_providers.push(Arc::new(RedditWsbProvider::new(Arc::clone(
                    &sentiment_http,
                ))));
            }
            let social_sentiment_service = Arc::new(SocialSentimentService::new(
                Arc::clone(&db),
                sentiment_providers,
            ));
            let social_sentiment_scheduler = Arc::new(SocialSentimentScheduler::new(
                Arc::clone(&social_sentiment_service),
                Arc::clone(&ibkr_state.tracker),
                Duration::from_secs(u64::from(config.social_sentiment.min_interval_minutes) * 60),
            ));
            if config.social_sentiment.enabled {
                let scheduler = Arc::clone(&social_sentiment_scheduler);
                tauri::async_runtime::spawn(async move {
                    let _handle = scheduler.spawn();
                    // Detached: handle drops at the end of run() since the
                    // scheduler stops cleanly when the runtime tears down.
                    // A future refactor can store the handle on IbkrState
                    // alongside the EOD/intraday handles for explicit
                    // start/stop commands.
                });
            }

            // Phase 4: candidate-universe upkeep. Sentiment-surge
            // refresh + decay sweep on a fixed cadence. Calendar-
            // agnostic — sentiment moves on weekends and the decay
            // sweep needs to run regardless of market hours so the
            // morning agent inbox isn't drowned in stale rows.
            let sentiment_surge_scanner = Arc::new(
                services::sentiment_surge_scanner::SentimentSurgeScanner::new(
                    Arc::clone(&db),
                    Arc::clone(&candidate_promoter),
                ),
            );
            let candidate_scheduler =
                Arc::new(services::candidate_scheduler::CandidateScheduler::new(
                    Arc::clone(&sentiment_surge_scanner),
                    Arc::clone(&candidate_universe),
                    Duration::from_secs(60 * 60),
                ));
            // Always spawn — decay needs to run independently of any
            // user opt-in, otherwise stale rows accumulate forever.
            // Sentiment-surge inside the tick is a no-op when
            // `social_sentiment` is disabled (no rows to spike against).
            {
                let scheduler = Arc::clone(&candidate_scheduler);
                tauri::async_runtime::spawn(async move {
                    let _handle = scheduler.spawn();
                });
            }

            // Phase 1 / Step 4: MCP read-only server. Listens on a local
            // socket (Unix) / named pipe (Windows) so Claude Code (and
            // other MCP clients) can drive interactive research sessions
            // through `bin/mcp-server`. Socket sits next to
            // `tracker.sqlite` in the OS app-local-data dir, matching
            // the bridge binary's default. Started here so we can grab
            // `Arc` clones of `ibkr_state.mcp_handle` and `llm_service`
            // before they're moved into `app.manage` below.
            // Phase 1 (assessment-MCP plan): persist IBKR fills so
            // assessment tooling (Phases 2/4/6) can query multi-day
            // history. The store sits behind `ProdAccountReader`'s
            // `executions(account, date)` so past-day queries are
            // served from SQLite while today still drains live; the
            // ingestor primes the store every 5 min during market
            // hours for off-band fills the live drain may miss.
            let executions_store = Arc::new(ExecutionsStore::new(Arc::clone(&db)));
            let executions_ingestor_fetcher: Arc<dyn LiveExecutionsFetcher> =
                Arc::clone(&ibkr_state.client) as Arc<dyn LiveExecutionsFetcher>;
            let executions_ingestor = Arc::new(ExecutionsIngestor::new(
                Arc::clone(&executions_store),
                executions_ingestor_fetcher,
            ));
            {
                let ingestor = Arc::clone(&executions_ingestor);
                tauri::async_runtime::spawn(async move {
                    let _handle = ingestor.spawn();
                });
            }
            // Detached: the StreamHandle stops cleanly when the tokio
            // runtime tears down. A future refactor can store it on
            // `IbkrState` if explicit start/stop is needed.

            let mcp_socket_path = db_dir.join("mcp.sock");
            let account_reader: Arc<dyn crate::mcp::ibkr_seam::AccountReader> =
                Arc::new(crate::mcp::ibkr_seam::ProdAccountReader::new(
                    Arc::clone(&ibkr_state.client)
                        as Arc<dyn crate::mcp::ibkr_seam::LiveAccountClient>,
                    Arc::clone(&executions_store),
                ));
            let trade_review_generator =
                Arc::new(crate::services::trade_reviews::TradeReviewGenerator::new(
                    Arc::clone(&llm_service),
                    Arc::clone(&account_reader),
                    Arc::clone(&db),
                ));
            let mcp_ibkr_client: Arc<dyn crate::mcp::ibkr_seam::AccountReader> =
                Arc::clone(&account_reader);
            let mcp_market_scanner: Arc<dyn MarketScanner> =
                Arc::clone(&ibkr_state.client) as Arc<dyn MarketScanner>;
            let mcp_handler = mcp::McpHandler::new(
                Arc::clone(&llm_service),
                Arc::clone(&ibkr_state.tracker),
                Arc::clone(&db),
                Arc::clone(&financial_service),
                Arc::clone(&fundamentals_provider),
                Arc::clone(&manual_fundamentals_store),
                Arc::clone(&news_provider),
                Arc::clone(&hist_service),
                Arc::clone(&quote_service),
                mcp_ibkr_client,
                Arc::clone(&auto_scanner_service),
                mcp_market_scanner,
                Arc::clone(&ibkr_state.event_emitter),
                Arc::clone(&social_sentiment_service),
                Arc::clone(&candidate_universe),
                Arc::clone(&candidate_promoter),
                Arc::clone(&ticker_primer),
                // v1: a single in-process MCP server, so every caller is
                // either Claude Code or the future agent loops talking
                // through the same `bin/mcp-server` bridge. Pin to
                // "interactive" until per-connection caller resolution
                // lands alongside the agent loops in Phase 5/6.
                "interactive".to_string(),
            );
            let mcp_server = mcp::server::McpServer::new(mcp_handler, mcp_socket_path);
            let mcp_state_handle = Arc::clone(&ibkr_state.mcp_handle);
            tauri::async_runtime::spawn(async move {
                match mcp_server.start().await {
                    Ok(handle) => {
                        *mcp_state_handle.write().await = Some(handle);
                    }
                    Err(e) => {
                        tracing::warn!("MCP server failed to start: {e}");
                    }
                }
            });

            app.manage(settings_state);
            app.manage(ibkr_state);
            app.manage(db);
            app.manage(hist_service);
            app.manage(financial_service);
            app.manage(fundamentals_provider);
            app.manage(manual_fundamentals_store);
            app.manage(news_provider);
            app.manage(av_call_ledger);
            app.manage(tracker_runner);
            app.manage(eod_scheduler);
            app.manage(intraday_scheduler);
            app.manage(llm_service);
            app.manage(thesis_generator);
            app.manage(news_interpreter);
            app.manage(daily_ranker);
            app.manage(auto_scanner_service);
            app.manage(auto_scanner_scheduler);
            app.manage(quote_service);
            app.manage(social_sentiment_service);
            app.manage(social_sentiment_scheduler);
            app.manage(candidate_universe);
            app.manage(candidate_promoter);
            app.manage(sentiment_surge_scanner);
            app.manage(candidate_scheduler);
            app.manage(ticker_primer);
            // Tauri commands (trades.rs) consume the same store-backed
            // AccountReader as the MCP server so desktop UI and Claude
            // Code see byte-identical past-day executions.
            app.manage(account_reader);
            app.manage(trade_review_generator);
            app.manage(risk_engine);
            app.manage(equity_snapshot_svc);
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
            ibkr::commands::ibkr_get_data_tier,
            ibkr::commands::ibkr_place_order,
            ibkr::commands::ibkr_get_executions,
            ibkr::commands::ibkr_get_executions_for_date,
            ibkr::commands::ibkr_get_fundamental_data,
            ibkr::commands::ibkr_get_quote,
            ibkr::commands::ibkr_generate_projections,
            ibkr::commands::ibkr_generate_projection_results,
            ibkr::commands::ibkr_get_cached_tickers,
            ibkr::commands::ibkr_start_scanner,
            ibkr::commands::ibkr_stop_scanner,
            ibkr::commands::tracker_fetch_bars,
            ibkr::commands::tracker_get_news,
            ibkr::commands::news_get_cached,
            ibkr::commands::tracker_add,
            ibkr::commands::tracker_remove,
            ibkr::commands::tracker_archive,
            ibkr::commands::tracker_unarchive,
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
            ibkr::commands::research_list_notes,
            ibkr::commands::research_get_note,
            ibkr::commands::research_get_agent_morning_pack,
            ibkr::commands::research_list_agent_morning_packs,
            ibkr::commands::research_list_mcp_audit,
            ibkr::commands::eval_calibration_stats,
            ibkr::commands::eval_cost_attribution,
            ibkr::commands::eval_prediction_history,
            ibkr::commands::auto_scanner_start,
            ibkr::commands::auto_scanner_stop,
            ibkr::commands::auto_scanner_get_config,
            ibkr::commands::auto_scanner_set_config,
            ibkr::commands::auto_scanner_run_once,
            ibkr::commands::social_get_latest,
            ibkr::commands::social_list_window,
            ibkr::commands::social_refresh_now,
            ibkr::commands::social_scheduler_status,
            ibkr::commands::candidates_list,
            ibkr::commands::candidates_promote,
            ibkr::commands::candidates_refresh_now,
            ibkr::commands::candidates_scheduler_status,
            ibkr::commands::get_trade_review,
            ibkr::commands::generate_trade_review,
            ibkr::commands::save_share_image_png,
            ibkr::commands::get_today_playbook,
            ibkr::commands::get_trader_profile,
            ibkr::commands::risk_get_config,
            ibkr::commands::risk_set_config,
            ibkr::commands::risk_recompute_setup,
            ibkr::commands::risk_refresh_equity,
            #[cfg(debug_assertions)]
            ibkr::commands::tracker_llm_smoke_test,
            config::commands::get_settings,
            config::commands::update_settings,
            config::commands::get_settings_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
