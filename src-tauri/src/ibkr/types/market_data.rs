use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketData {
    pub symbol: String,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub last: Option<f64>,
    pub volume: Option<i64>,
    pub timestamp: i64,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickData {
    pub tick_type: TickType,
    pub price: Option<f64>,
    pub size: Option<i32>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TickType {
    BidSize,
    Bid,
    Ask,
    AskSize,
    Last,
    LastSize,
    High,
    Low,
    Volume,
    Close,
    Open,
    Halted,
    BidOptionComputation,
    AskOptionComputation,
}

// News Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsArticle {
    pub article_id: String,
    pub headline: String,
    pub article_type: String,
    pub article_text: String,
    pub language: String,
    pub provider_code: String,
    pub provider_name: String,
    pub timestamp: i64,
}