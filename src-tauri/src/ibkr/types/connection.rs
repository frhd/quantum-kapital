use crate::config::IbkrConfig;
use serde::{Deserialize, Serialize};

/// IBKR market data type. Mirrors `ibapi::market_data::MarketDataType`.
///
/// Defaults to `Delayed` so accounts without real-time market data subscriptions
/// (e.g. paper-trading) still receive ticks instead of error 354.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MarketDataType {
    Live,
    Frozen,
    #[default]
    Delayed,
    DelayedFrozen,
}

impl From<MarketDataType> for ibapi::market_data::MarketDataType {
    fn from(value: MarketDataType) -> Self {
        match value {
            MarketDataType::Live => ibapi::market_data::MarketDataType::Realtime,
            MarketDataType::Frozen => ibapi::market_data::MarketDataType::Frozen,
            MarketDataType::Delayed => ibapi::market_data::MarketDataType::Delayed,
            MarketDataType::DelayedFrozen => ibapi::market_data::MarketDataType::DelayedFrozen,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub client_id: i32,
    #[serde(default)]
    pub market_data_type: MarketDataType,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 4004,
            client_id: 100,
            market_data_type: MarketDataType::default(),
        }
    }
}

impl From<IbkrConfig> for ConnectionConfig {
    fn from(config: IbkrConfig) -> Self {
        Self {
            host: config.default_host,
            port: config.default_port,
            client_id: config.default_client_id,
            market_data_type: MarketDataType::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub server_time: Option<String>,
    pub client_id: i32,
}
