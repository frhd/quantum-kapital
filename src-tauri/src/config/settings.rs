use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub ibkr: IbkrConfig,
    pub logging: LoggingConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbkrConfig {
    pub default_host: String,
    pub default_port: u16,
    pub default_client_id: i32,
    pub connection_timeout_ms: u64,
    pub reconnect_interval_ms: u64,
    pub max_reconnect_attempts: u32,
    pub rate_limit_per_second: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file_path: Option<PathBuf>,
    pub max_file_size_mb: u64,
    pub max_files: u32,
    pub console_output: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub default_refresh_interval_ms: u64,
    pub show_notifications: bool,
    pub auto_save_layout: bool,
}

impl Default for IbkrConfig {
    fn default() -> Self {
        Self {
            default_host: "127.0.0.1".to_string(),
            default_port: 4002,
            default_client_id: 100,
            connection_timeout_ms: 30000,
            reconnect_interval_ms: 5000,
            max_reconnect_attempts: 3,
            rate_limit_per_second: 50,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file_path: None,
            max_file_size_mb: 10,
            max_files: 5,
            console_output: true,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            default_refresh_interval_ms: 1000,
            show_notifications: true,
            auto_save_layout: true,
        }
    }
}

#[allow(dead_code)]
impl AppConfig {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        // Try to load from config file, fall back to defaults
        // This is a placeholder - implement actual config loading logic
        Ok(Self::default())
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Save config to file
        // This is a placeholder - implement actual config saving logic
        Ok(())
    }
}
