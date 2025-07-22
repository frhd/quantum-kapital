use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub client_id: i32,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 4002,
            client_id: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub account_id: String,
    pub account_type: String,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub account: String,
    pub tag: String,
    pub value: String,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub account: String,
    pub symbol: String,
    pub position: f64,
    pub average_cost: f64,
    pub market_price: f64,
    pub market_value: f64,
    pub unrealized_pnl: f64,
    pub realized_pnl: f64,
    pub contract_type: String, // "STK" for stocks, "OPT" for options, etc.
    pub currency: String,
    pub exchange: String,
    pub local_symbol: String, // For options, this includes strike and expiry
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub server_time: Option<String>,
    pub client_id: i32,
}

// Contract Types for different securities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDetails {
    pub symbol: String,
    pub sec_type: SecurityType,
    pub exchange: String,
    pub primary_exchange: String,
    pub currency: String,
    pub local_symbol: String,
    pub trading_class: String,
    pub contract_id: i32,
    pub min_tick: f64,
    pub multiplier: String,
    pub price_magnifier: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityType {
    Stock,
    Option,
    Future,
    Forex,
    Combo,
    Warrant,
    Bond,
    Commodity,
    News,
    Fund,
}

// Market Data Types
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

// Historical Data Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalDataRequest {
    pub symbol: String,
    pub end_date_time: String,
    pub duration: String,
    pub bar_size: BarSize,
    pub what_to_show: WhatToShow,
    pub use_rth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BarSize {
    Sec1,
    Sec5,
    Sec15,
    Sec30,
    Min1,
    Min2,
    Min3,
    Min5,
    Min15,
    Min20,
    Min30,
    Hour1,
    Day1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WhatToShow {
    Trades,
    Midpoint,
    Bid,
    Ask,
    BidAsk,
    HistoricalVolatility,
    OptionImpliedVolatility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalBar {
    pub time: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: i64,
    pub wap: f64,
    pub count: i32,
}

// Account and Portfolio Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountValue {
    pub key: String,
    pub value: String,
    pub currency: String,
    pub account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioPosition {
    pub contract: ContractDetails,
    pub position: f64,
    pub market_price: f64,
    pub market_value: f64,
    pub average_cost: f64,
    pub unrealized_pnl: f64,
    pub realized_pnl: f64,
    pub account: String,
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

// Scanner Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerSubscription {
    pub number_of_rows: i32,
    pub instrument: String,
    pub location_code: String,
    pub scan_code: String,
    pub above_price: Option<f64>,
    pub below_price: Option<f64>,
    pub above_volume: Option<i32>,
    pub market_cap_above: Option<f64>,
    pub market_cap_below: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerData {
    pub rank: i32,
    pub contract: ContractDetails,
    pub distance: String,
    pub benchmark: String,
    pub projection: String,
    pub legs: String,
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