use crate::events::{AppEvent, EventEmitter};
use crate::ibkr::client::IbkrClient;
use crate::ibkr::types::{ConnectionConfig, MarketDataSnapshot, Position};
use crate::middleware::RateLimiter;
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

#[allow(dead_code)]
pub struct MarketDataCache {
    snapshots: HashMap<String, MarketDataSnapshot>,
    last_updated: HashMap<String, chrono::DateTime<chrono::Utc>>,
}

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
    pub rate_limiter: Arc<RateLimiter>,
    pub connection_info: Arc<RwLock<ConnectionInfo>>,
    pub market_data_cache: Arc<RwLock<MarketDataCache>>,
    pub position_cache: Arc<RwLock<PositionCache>>,
    pub config: Arc<RwLock<ConnectionConfig>>,
}

#[allow(dead_code)]
impl IbkrState {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            client: Arc::new(IbkrClient::new(config.clone())),
            event_emitter: Arc::new(EventEmitter::new()),
            rate_limiter: Arc::new(RateLimiter::new(50)), // Default 50 requests per second
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
            config: Arc::new(RwLock::new(config)),
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
        let _ = self.event_emitter.emit(AppEvent::ConnectionStatusChanged {
            connected,
            message: if connected {
                "Connected to IBKR".to_string()
            } else {
                "Disconnected from IBKR".to_string()
            },
        }).await;
    }

    pub async fn increment_retry_count(&self) {
        let mut info = self.connection_info.write().await;
        info.retry_count += 1;
    }

    pub async fn cache_market_data(&self, symbol: String, snapshot: MarketDataSnapshot) {
        let mut cache = self.market_data_cache.write().await;
        cache.snapshots.insert(symbol.clone(), snapshot.clone());
        cache.last_updated.insert(symbol.clone(), chrono::Utc::now());

        // Emit market data update event
        let _ = self.event_emitter.emit(AppEvent::MarketDataUpdate {
            symbol,
            data: serde_json::to_value(&snapshot).unwrap_or_default(),
        }).await;
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
}

impl Default for MarketDataCache {
    fn default() -> Self {
        Self {
            snapshots: HashMap::new(),
            last_updated: HashMap::new(),
        }
    }
}

impl Default for PositionCache {
    fn default() -> Self {
        Self {
            positions: HashMap::new(),
            last_refresh: None,
        }
    }
}