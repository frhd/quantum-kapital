use serde::{Deserialize, Serialize};

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