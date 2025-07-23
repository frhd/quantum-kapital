use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AppEvent {
    // Connection events
    ConnectionStatusChanged {
        connected: bool,
        message: String,
    },
    ConnectionError {
        error: String,
    },

    // Account events
    AccountUpdate {
        account_id: String,
        data: serde_json::Value,
    },
    AccountsListChanged {
        accounts: Vec<String>,
    },

    // Market data events
    MarketDataUpdate {
        symbol: String,
        data: serde_json::Value,
    },
    MarketDataSubscribed {
        symbol: String,
    },
    MarketDataUnsubscribed {
        symbol: String,
    },

    // Order events
    OrderPlaced {
        order_id: i32,
        symbol: String,
    },
    OrderFilled {
        order_id: i32,
        filled_qty: f64,
    },
    OrderCancelled {
        order_id: i32,
    },
    OrderError {
        order_id: Option<i32>,
        error: String,
    },

    // Position events
    PositionUpdate {
        symbol: String,
        position: f64,
    },
    PositionsRefreshed,

    // System events
    RateLimitWarning {
        remaining: u32,
    },
    SystemError {
        error: String,
    },
}

pub struct EventEmitter {
    app_handle: Arc<RwLock<Option<AppHandle>>>,
}

#[allow(dead_code)]
impl EventEmitter {
    pub fn new() -> Self {
        Self {
            app_handle: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_app_handle(&self, handle: AppHandle) {
        let mut app_handle = self.app_handle.write().await;
        *app_handle = Some(handle);
    }

    pub async fn emit(&self, event: AppEvent) -> Result<(), String> {
        let app_handle = self.app_handle.read().await;

        if let Some(handle) = app_handle.as_ref() {
            let event_name = match &event {
                AppEvent::ConnectionStatusChanged { .. } => "connection-status-changed",
                AppEvent::ConnectionError { .. } => "connection-error",
                AppEvent::AccountUpdate { .. } => "account-update",
                AppEvent::AccountsListChanged { .. } => "accounts-list-changed",
                AppEvent::MarketDataUpdate { .. } => "market-data-update",
                AppEvent::MarketDataSubscribed { .. } => "market-data-subscribed",
                AppEvent::MarketDataUnsubscribed { .. } => "market-data-unsubscribed",
                AppEvent::OrderPlaced { .. } => "order-placed",
                AppEvent::OrderFilled { .. } => "order-filled",
                AppEvent::OrderCancelled { .. } => "order-cancelled",
                AppEvent::OrderError { .. } => "order-error",
                AppEvent::PositionUpdate { .. } => "position-update",
                AppEvent::PositionsRefreshed => "positions-refreshed",
                AppEvent::RateLimitWarning { .. } => "rate-limit-warning",
                AppEvent::SystemError { .. } => "system-error",
            };

            handle
                .emit(event_name, &event)
                .map_err(|e| format!("Failed to emit event: {e}"))?;

            Ok(())
        } else {
            Err("App handle not initialized".to_string())
        }
    }

    pub async fn emit_to_window(&self, window: &str, event: AppEvent) -> Result<(), String> {
        let app_handle = self.app_handle.read().await;

        if let Some(handle) = app_handle.as_ref() {
            if let Some(window) = handle.get_webview_window(window) {
                let event_name = match &event {
                    AppEvent::ConnectionStatusChanged { .. } => "connection-status-changed",
                    AppEvent::ConnectionError { .. } => "connection-error",
                    AppEvent::AccountUpdate { .. } => "account-update",
                    AppEvent::AccountsListChanged { .. } => "accounts-list-changed",
                    AppEvent::MarketDataUpdate { .. } => "market-data-update",
                    AppEvent::MarketDataSubscribed { .. } => "market-data-subscribed",
                    AppEvent::MarketDataUnsubscribed { .. } => "market-data-unsubscribed",
                    AppEvent::OrderPlaced { .. } => "order-placed",
                    AppEvent::OrderFilled { .. } => "order-filled",
                    AppEvent::OrderCancelled { .. } => "order-cancelled",
                    AppEvent::OrderError { .. } => "order-error",
                    AppEvent::PositionUpdate { .. } => "position-update",
                    AppEvent::PositionsRefreshed => "positions-refreshed",
                    AppEvent::RateLimitWarning { .. } => "rate-limit-warning",
                    AppEvent::SystemError { .. } => "system-error",
                };

                window
                    .emit(event_name, &event)
                    .map_err(|e| format!("Failed to emit event to window: {e}"))?;

                Ok(())
            } else {
                Err(format!("Window '{window}' not found"))
            }
        } else {
            Err("App handle not initialized".to_string())
        }
    }
}
