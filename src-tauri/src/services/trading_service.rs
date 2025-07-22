use crate::events::AppEvent;
use crate::ibkr::state::IbkrState;
use crate::ibkr::types::{OrderAction, OrderRequest, OrderStatus, OrderType};
use crate::utils;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(dead_code)]
pub struct TradingService {
    state: Arc<IbkrState>,
    pending_orders: Arc<RwLock<HashMap<i32, OrderTracker>>>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct OrderTracker {
    order_id: i32,
    request: OrderRequest,
    status: OrderStatus,
    placed_at: chrono::DateTime<chrono::Utc>,
    last_update: chrono::DateTime<chrono::Utc>,
}

#[allow(dead_code)]
impl TradingService {
    pub fn new(state: Arc<IbkrState>) -> Self {
        Self {
            state,
            pending_orders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Place an order with validation and tracking
    pub async fn place_order_with_validation(
        &self,
        order: OrderRequest,
    ) -> Result<OrderPlacementResult, String> {
        // Validate order
        self.validate_order(&order)?;

        // Check rate limit
        match self.state.rate_limiter.check_and_update("place_order").await {
            Ok(_) => {}
            Err(e) => return Err(e),
        }

        // Place the order
        match self.state.client.place_order(order.clone()).await {
            Ok(order_id) => {
                // Track the order
                let tracker = OrderTracker {
                    order_id,
                    request: order.clone(),
                    status: OrderStatus {
                        order_id,
                        status: "Submitted".to_string(),
                        filled: 0.0,
                        remaining: order.quantity,
                        average_fill_price: None,
                    },
                    placed_at: chrono::Utc::now(),
                    last_update: chrono::Utc::now(),
                };

                let mut orders = self.pending_orders.write().await;
                orders.insert(order_id, tracker);

                // Emit order placed event
                let _ = self.state.event_emitter.emit(AppEvent::OrderPlaced {
                    order_id,
                    symbol: order.symbol.clone(),
                }).await;

                Ok(OrderPlacementResult {
                    order_id,
                    symbol: order.symbol,
                    action: order.action,
                    quantity: order.quantity,
                    order_type: order.order_type,
                    price: order.price,
                    estimated_commission: self.estimate_commission(order.quantity),
                    placed_at: utils::current_timestamp_ms(),
                })
            }
            Err(e) => {
                // Emit order error event
                let _ = self.state.event_emitter.emit(AppEvent::OrderError {
                    order_id: None,
                    error: e.to_string(),
                }).await;

                Err(e.to_string())
            }
        }
    }

    /// Validate order parameters
    fn validate_order(&self, order: &OrderRequest) -> Result<(), String> {
        // Validate symbol
        if !utils::is_valid_symbol(&order.symbol) {
            return Err(format!("Invalid symbol: {}", order.symbol));
        }

        // Validate quantity
        if order.quantity <= 0.0 {
            return Err("Quantity must be positive".to_string());
        }

        // Validate price for limit orders
        match order.order_type {
            OrderType::Limit | OrderType::StopLimit => {
                if order.price.is_none() || order.price.unwrap() <= 0.0 {
                    return Err("Limit orders require a positive price".to_string());
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Estimate commission for an order
    fn estimate_commission(&self, quantity: f64) -> f64 {
        // Simple commission model: $0.005 per share, min $1
        let commission = quantity * 0.005;
        commission.max(1.0)
    }

    /// Cancel an order
    pub async fn cancel_order(&self, order_id: i32) -> Result<(), String> {
        // Check if we're tracking this order
        let orders = self.pending_orders.read().await;
        if !orders.contains_key(&order_id) {
            return Err(format!("Order {} not found in tracking", order_id));
        }
        drop(orders);

        // TODO: Implement actual order cancellation when API supports it
        
        // Remove from tracking
        let mut orders = self.pending_orders.write().await;
        orders.remove(&order_id);

        // Emit cancellation event
        let _ = self.state.event_emitter.emit(AppEvent::OrderCancelled {
            order_id,
        }).await;

        Ok(())
    }

    /// Get status of pending orders
    pub async fn get_pending_orders(&self) -> Vec<OrderStatusReport> {
        let orders = self.pending_orders.read().await;
        
        orders
            .values()
            .map(|tracker| OrderStatusReport {
                order_id: tracker.order_id,
                symbol: tracker.request.symbol.clone(),
                action: tracker.request.action.clone(),
                quantity: tracker.request.quantity,
                order_type: tracker.request.order_type.clone(),
                status: tracker.status.status.clone(),
                filled: tracker.status.filled,
                remaining: tracker.status.remaining,
                average_fill_price: tracker.status.average_fill_price,
                placed_at: tracker.placed_at.timestamp_millis(),
                duration_ms: chrono::Utc::now()
                    .signed_duration_since(tracker.placed_at)
                    .num_milliseconds(),
            })
            .collect()
    }

    /// Update order status (would be called by event handlers)
    pub async fn update_order_status(&self, order_id: i32, status: OrderStatus) {
        let mut orders = self.pending_orders.write().await;
        
        if let Some(tracker) = orders.get_mut(&order_id) {
            let was_pending = tracker.status.remaining > 0.0;
            tracker.status = status.clone();
            tracker.last_update = chrono::Utc::now();

            // Check if order is filled
            if was_pending && status.remaining == 0.0 {
                // Emit fill event
                let _ = self.state.event_emitter.emit(AppEvent::OrderFilled {
                    order_id,
                    filled_qty: status.filled,
                }).await;

                // Remove from pending orders after a delay
                let _order_id_to_remove = order_id;
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    // Note: In real implementation, would need access to self
                });
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderPlacementResult {
    pub order_id: i32,
    pub symbol: String,
    pub action: OrderAction,
    pub quantity: f64,
    pub order_type: OrderType,
    pub price: Option<f64>,
    pub estimated_commission: f64,
    pub placed_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderStatusReport {
    pub order_id: i32,
    pub symbol: String,
    pub action: OrderAction,
    pub quantity: f64,
    pub order_type: OrderType,
    pub status: String,
    pub filled: f64,
    pub remaining: f64,
    pub average_fill_price: Option<f64>,
    pub placed_at: i64,
    pub duration_ms: i64,
}