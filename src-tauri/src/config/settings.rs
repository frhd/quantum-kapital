use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub ibkr: IbkrConfig,
    pub logging: LoggingConfig,
    pub ui: UiConfig,
    pub api: ApiConfig,
    pub google_sheets: GoogleSheetsConfig,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub alpha_vantage_api_key: Option<String>, // Alpha Vantage API key
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleSheetsConfig {
    pub spreadsheet_id: Option<String>,
    pub spreadsheet_name: String,
    pub auto_export: bool,
    pub last_export_timestamp: Option<String>,
}

impl Default for IbkrConfig {
    fn default() -> Self {
        Self {
            default_host: "127.0.0.1".to_string(),
            default_port: 4004,
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

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            alpha_vantage_api_key: std::env::var("ALPHA_VANTAGE_API_KEY").ok(),
        }
    }
}

impl Default for GoogleSheetsConfig {
    fn default() -> Self {
        Self {
            spreadsheet_id: None,
            spreadsheet_name: "Quantum Kapital Analysis".to_string(),
            auto_export: false,
            last_export_timestamp: None,
        }
    }
}

#[allow(dead_code)]
impl AppConfig {
    /// Get the path to the settings file
    pub fn settings_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let config_dir = dirs::config_dir().ok_or("Could not find config directory")?;
        let app_dir = config_dir.join("quantum-kapital");
        Ok(app_dir.join("settings.json"))
    }

    /// Load settings from disk, or return defaults if file doesn't exist
    pub async fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let settings_path = Self::settings_path()?;

        if settings_path.exists() {
            let contents = fs::read_to_string(&settings_path).await?;
            let config: AppConfig = serde_json::from_str(&contents)?;
            Ok(config)
        } else {
            // Return default settings if file doesn't exist
            Ok(Self::default())
        }
    }

    /// Save settings to disk
    pub async fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let settings_path = Self::settings_path()?;

        // Ensure directory exists
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Serialize settings with pretty formatting
        let json = serde_json::to_string_pretty(self)?;

        // Write to file
        fs::write(&settings_path, json).await?;

        Ok(())
    }

    /// Load synchronously (for initial app setup)
    pub fn load_sync() -> Result<Self, Box<dyn std::error::Error>> {
        let settings_path = Self::settings_path()?;

        if settings_path.exists() {
            let contents = std::fs::read_to_string(&settings_path)?;
            let config: AppConfig = serde_json::from_str(&contents)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }
}
