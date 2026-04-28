use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDataSnapshot {
    pub symbol: String,
    pub bid_price: Option<f64>,
    pub bid_size: Option<i32>,
    pub ask_price: Option<f64>,
    pub ask_size: Option<i32>,
    pub last_price: Option<f64>,
    pub last_size: Option<i32>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub volume: Option<i32>,
    pub close: Option<f64>,
    pub open: Option<f64>,
    pub timestamp: i64,
}
