use super::settings::AppConfig;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Global settings state
pub struct SettingsState {
    pub config: Arc<RwLock<AppConfig>>,
}

impl SettingsState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
        }
    }
}

/// Get all settings
#[tauri::command]
pub async fn get_settings(state: State<'_, SettingsState>) -> Result<AppConfig, String> {
    let config = state.config.read().await;
    Ok(config.clone())
}

/// Update settings
#[tauri::command]
pub async fn update_settings(
    settings: AppConfig,
    state: State<'_, SettingsState>,
) -> Result<(), String> {
    let mut config = state.config.write().await;
    *config = settings.clone();

    // Save to disk
    settings
        .save()
        .await
        .map_err(|e| format!("Failed to save settings: {e}"))?;

    Ok(())
}

/// Get settings file path (for debugging)
#[tauri::command]
pub async fn get_settings_path() -> Result<String, String> {
    AppConfig::settings_path()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("Failed to get settings path: {e}"))
}
