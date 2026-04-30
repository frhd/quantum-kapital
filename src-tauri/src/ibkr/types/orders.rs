use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionSide {
    Bought,
    Sold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbkrExecution {
    pub symbol: String,
    pub side: ExecutionSide,
    pub qty: f64,
    pub avg_price: f64,
    pub exec_time: DateTime<Utc>,
    pub order_id: i32,
    pub exec_id: String,
}

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

#[cfg(test)]
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
