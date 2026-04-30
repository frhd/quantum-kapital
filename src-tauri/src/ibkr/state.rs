use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::client::{IbkrClient, StreamHandle};
use crate::ibkr::types::{ConnectionConfig, MarketDataSnapshot, Position, ScannerSubscription};
use crate::services::eod_scheduler::EodScheduler;
use crate::services::intraday_scheduler::IntradayScheduler;
use crate::services::llm_service::LlmService;
use crate::services::tracker_service::TrackerService;
use crate::services::tracker_state_machine::TrackerStateMachine;
use crate::storage::Db;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ConnectionInfo {
    pub connected: bool,
    pub last_attempt: Option<chrono::DateTime<chrono::Utc>>,
    pub retry_count: u32,
}

#[derive(Default)]
#[allow(dead_code)]
pub struct MarketDataCache {
    snapshots: HashMap<String, MarketDataSnapshot>,
    last_updated: HashMap<String, chrono::DateTime<chrono::Utc>>,
}

#[derive(Default)]
#[allow(dead_code)]
pub struct PositionCache {
    positions: HashMap<String, Position>,
    last_refresh: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct IbkrState {
    pub client: Arc<IbkrClient>,
    pub event_emitter: Arc<EventEmitter>,
    pub connection_info: Arc<RwLock<ConnectionInfo>>,
    pub market_data_cache: Arc<RwLock<MarketDataCache>>,
    pub position_cache: Arc<RwLock<PositionCache>>,
    pub config: Arc<RwLock<ConnectionConfig>>,
    pub daily_pnl_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub scanner_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub eod_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub intraday_handle: Arc<RwLock<Option<StreamHandle>>>,
    pub db: Arc<Db>,
    pub tracker: Arc<TrackerService>,
    pub state_machine: Arc<TrackerStateMachine>,
    pub llm: Arc<LlmService>,
}

#[allow(dead_code)]
impl IbkrState {
    pub fn new(config: ConnectionConfig, db: Arc<Db>, llm: Arc<LlmService>) -> Self {
        let config_arc = Arc::new(RwLock::new(config));
        let event_emitter = Arc::new(EventEmitter::new());
        let tracker = Arc::new(TrackerService::new(Arc::clone(&db)));
        let state_machine = Arc::new(TrackerStateMachine::new(
            Arc::clone(&db),
            Arc::clone(&tracker),
            Arc::clone(&event_emitter),
        ));
        Self {
            client: Arc::new(IbkrClient::with_shared_config(Arc::clone(&config_arc))),
            event_emitter,
            connection_info: Arc::new(RwLock::new(ConnectionInfo {
                connected: false,
                last_attempt: None,
                retry_count: 0,
            })),
            market_data_cache: Arc::new(RwLock::new(MarketDataCache {
                snapshots: HashMap::new(),
                last_updated: HashMap::new(),
            })),
            position_cache: Arc::new(RwLock::new(PositionCache {
                positions: HashMap::new(),
                last_refresh: None,
            })),
            config: config_arc,
            daily_pnl_handle: Arc::new(RwLock::new(None)),
            scanner_handle: Arc::new(RwLock::new(None)),
            eod_handle: Arc::new(RwLock::new(None)),
            intraday_handle: Arc::new(RwLock::new(None)),
            db,
            tracker,
            state_machine,
            llm,
        }
    }

    pub async fn start_daily_pnl(&self, account: &str) -> Result<(), String> {
        // Replace any existing subscription
        self.stop_daily_pnl().await;

        let handle = self
            .client
            .start_daily_pnl_stream(account, Arc::clone(&self.event_emitter))
            .await
            .map_err(|e| e.to_string())?;
        *self.daily_pnl_handle.write().await = Some(handle);
        Ok(())
    }

    pub async fn stop_daily_pnl(&self) {
        let handle = self.daily_pnl_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    pub async fn start_scanner(&self, opts: ScannerSubscription) -> Result<(), String> {
        // Replace any existing subscription
        self.stop_scanner().await;

        let handle = self
            .client
            .start_scanner_stream(opts, Arc::clone(&self.event_emitter))
            .await
            .map_err(|e| e.to_string())?;
        *self.scanner_handle.write().await = Some(handle);
        Ok(())
    }

    pub async fn stop_scanner(&self) {
        let handle = self.scanner_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    pub async fn start_eod_scheduler(&self, scheduler: Arc<EodScheduler>) -> Result<(), String> {
        // Replace any existing scheduler — same pattern as start_scanner.
        self.stop_eod_scheduler().await;
        let handle = scheduler.spawn();
        *self.eod_handle.write().await = Some(handle);
        Ok(())
    }

    pub async fn stop_eod_scheduler(&self) {
        let handle = self.eod_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    pub async fn start_intraday_scheduler(
        &self,
        scheduler: Arc<IntradayScheduler>,
    ) -> Result<(), String> {
        // Replace any existing scheduler — same pattern as start_scanner.
        self.stop_intraday_scheduler().await;
        let handle = scheduler.spawn();
        *self.intraday_handle.write().await = Some(handle);
        Ok(())
    }

    pub async fn stop_intraday_scheduler(&self) {
        let handle = self.intraday_handle.write().await.take();
        if let Some(handle) = handle {
            handle.stop().await;
        }
    }

    pub async fn update_connection_status(&self, connected: bool) {
        let mut info = self.connection_info.write().await;
        info.connected = connected;
        info.last_attempt = Some(chrono::Utc::now());

        if connected {
            info.retry_count = 0;
        }

        // Emit connection status event
        let _ = self
            .event_emitter
            .emit(AppEvent::ConnectionStatusChanged {
                connected,
                message: if connected {
                    "Connected to IBKR".to_string()
                } else {
                    "Disconnected from IBKR".to_string()
                },
            })
            .await;
    }

    pub async fn increment_retry_count(&self) {
        let mut info = self.connection_info.write().await;
        info.retry_count += 1;
    }

    pub async fn cache_market_data(&self, symbol: String, snapshot: MarketDataSnapshot) {
        let mut cache = self.market_data_cache.write().await;
        cache.snapshots.insert(symbol.clone(), snapshot.clone());
        cache
            .last_updated
            .insert(symbol.clone(), chrono::Utc::now());

        // Emit market data update event
        let _ = self
            .event_emitter
            .emit(AppEvent::MarketDataUpdate {
                symbol,
                data: serde_json::to_value(&snapshot).unwrap_or_default(),
            })
            .await;
    }

    pub async fn get_cached_market_data(&self, symbol: &str) -> Option<MarketDataSnapshot> {
        let cache = self.market_data_cache.read().await;
        cache.snapshots.get(symbol).cloned()
    }

    pub async fn cache_positions(&self, positions: Vec<Position>) {
        let mut cache = self.position_cache.write().await;
        cache.positions.clear();

        for position in positions {
            cache.positions.insert(position.symbol.clone(), position);
        }

        cache.last_refresh = Some(chrono::Utc::now());

        // Emit positions refreshed event
        let _ = self.event_emitter.emit(AppEvent::PositionsRefreshed).await;
    }

    pub async fn get_cached_positions(&self) -> Vec<Position> {
        let cache = self.position_cache.read().await;
        cache.positions.values().cloned().collect()
    }

    pub async fn is_cache_stale(&self, cache_duration_secs: i64) -> bool {
        let cache = self.position_cache.read().await;

        if let Some(last_refresh) = cache.last_refresh {
            let elapsed = chrono::Utc::now()
                .signed_duration_since(last_refresh)
                .num_seconds();
            elapsed > cache_duration_secs
        } else {
            true
        }
    }

    pub async fn increment_client_id(&self) {
        let mut config = self.config.write().await;
        config.client_id += 1;
        tracing::info!("🔵 CLIENT ID INCREMENTED TO: {}", config.client_id);
    }
}
