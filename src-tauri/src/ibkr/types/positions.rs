use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Option / future expiry. `YYYYMMDD` (last trading day) or `YYYYMM`
    /// (contract month). `None` for stocks. Sourced from
    /// `Contract::last_trade_date_or_contract_month`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry: Option<String>,
    /// Option strike price. `None` for non-option instruments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<f64>,
    /// Option right — `"C"` (call) or `"P"` (put). `None` for
    /// non-options. Pass-through from IBKR (sometimes `CALL` / `PUT`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
    /// Contract multiplier (e.g. `"100"` for standard equity options).
    /// `None` when IBKR didn't report one (typical for stocks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<String>,
}

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
