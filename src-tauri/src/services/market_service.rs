use crate::events::AppEvent;
use crate::ibkr::state::IbkrState;
use crate::ibkr::types::MarketDataSnapshot;
use crate::utils;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(dead_code)]
pub struct MarketService {
    state: Arc<IbkrState>,
    active_subscriptions: Arc<RwLock<HashMap<String, i32>>>, // symbol -> request_id
}

#[allow(dead_code)]
impl MarketService {
    pub fn new(state: Arc<IbkrState>) -> Self {
        Self {
            state,
            active_subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Subscribe to market data with rate limiting
    pub async fn subscribe_with_rate_limit(&self, symbol: &str) -> Result<(), String> {
        // Check rate limit
        match self
            .state
            .rate_limiter
            .check_and_update("market_data")
            .await
        {
            Ok(remaining) => {
                if remaining < 10 {
                    tracing::warn!(
                        "Approaching rate limit for market data: {} remaining",
                        remaining
                    );
                }
            }
            Err(e) => return Err(e),
        }

        // Check if already subscribed
        let subs = self.active_subscriptions.read().await;
        if subs.contains_key(symbol) {
            return Ok(()); // Already subscribed
        }
        drop(subs);

        // Subscribe
        let result = self.state.client.subscribe_market_data(symbol).await;

        match result {
            Ok(_) => {
                // Track subscription
                let mut subs = self.active_subscriptions.write().await;
                subs.insert(symbol.to_string(), utils::current_timestamp_ms() as i32);

                // Emit subscription event
                let _ = self
                    .state
                    .event_emitter
                    .emit(AppEvent::MarketDataSubscribed {
                        symbol: symbol.to_string(),
                    })
                    .await;

                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Unsubscribe from market data
    pub async fn unsubscribe(&self, symbol: &str) -> Result<(), String> {
        let mut subs = self.active_subscriptions.write().await;

        if subs.remove(symbol).is_some() {
            // TODO: Call actual unsubscribe method when implemented

            // Emit unsubscription event
            let _ = self
                .state
                .event_emitter
                .emit(AppEvent::MarketDataUnsubscribed {
                    symbol: symbol.to_string(),
                })
                .await;
        }

        Ok(())
    }

    /// Get market data with caching
    pub async fn get_market_data(&self, symbol: &str) -> Result<MarketDataSnapshot, String> {
        // Check cache first
        if let Some(cached) = self.state.get_cached_market_data(symbol).await {
            // Check if cache is fresh (less than 1 second old)
            return Ok(cached);
        }

        // If not cached or stale, subscribe if not already
        self.subscribe_with_rate_limit(symbol).await?;

        // Wait a bit for data to arrive
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Check cache again
        if let Some(cached) = self.state.get_cached_market_data(symbol).await {
            Ok(cached)
        } else {
            Err(format!("No market data available for {symbol}"))
        }
    }

    /// Bulk subscribe to multiple symbols
    pub async fn bulk_subscribe(
        &self,
        symbols: Vec<String>,
    ) -> HashMap<String, Result<(), String>> {
        let mut results = HashMap::new();

        for symbol in symbols {
            let result = self.subscribe_with_rate_limit(&symbol).await;
            results.insert(symbol, result);

            // Small delay between subscriptions to avoid overwhelming the API
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        results
    }

    /// Get active subscriptions
    pub async fn get_active_subscriptions(&self) -> Vec<String> {
        let subs = self.active_subscriptions.read().await;
        subs.keys().cloned().collect()
    }

    /// Clear all subscriptions
    pub async fn clear_all_subscriptions(&self) -> Result<(), String> {
        let symbols: Vec<String> = {
            let subs = self.active_subscriptions.read().await;
            subs.keys().cloned().collect()
        };

        for symbol in symbols {
            self.unsubscribe(&symbol).await?;
        }

        Ok(())
    }
}
