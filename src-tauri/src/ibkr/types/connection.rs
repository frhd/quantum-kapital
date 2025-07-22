use serde::{Deserialize, Serialize};
use crate::config::IbkrConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub client_id: i32,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 4002,
            client_id: 100,
        }
    }
}

impl From<IbkrConfig> for ConnectionConfig {
    fn from(config: IbkrConfig) -> Self {
        Self {
            host: config.default_host,
            port: config.default_port,
            client_id: config.default_client_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub server_time: Option<String>,
    pub client_id: i32,
}