use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,
    pub action: OrderAction,
    pub quantity: f64,
    pub order_type: OrderType,
    pub price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderAction {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderStatus {
    pub order_id: i32,
    pub status: String,
    pub filled: f64,
    pub remaining: f64,
    pub average_fill_price: Option<f64>,
}

// Execution and Commission Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub exec_id: String,
    pub time: String,
    pub account: String,
    pub exchange: String,
    pub side: String,
    pub shares: f64,
    pub price: f64,
    pub perm_id: i32,
    pub client_id: i32,
    pub order_id: i32,
    pub liquidation: bool,
    pub cum_qty: f64,
    pub avg_price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommissionReport {
    pub exec_id: String,
    pub commission: f64,
    pub currency: String,
    pub realized_pnl: f64,
    pub yield_val: f64,
    pub yield_redemption_date: i32,
}
