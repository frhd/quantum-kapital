use serde::{Deserialize, Serialize};

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