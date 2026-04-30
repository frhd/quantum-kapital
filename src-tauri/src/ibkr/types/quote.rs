use serde::{Deserialize, Serialize};

/// A live, never-cached, UI-shaped quote. Sourced from
/// `MarketDataSnapshot` via `QuoteService`. Distinct from
/// `MarketDataSnapshot` because the UI only needs four fields and
/// because future quote sources need not match the snapshot shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub symbol: String,
    /// Last traded price (regular or delayed, depending on TWS data
    /// permissions). `None` if no last tick was received before the
    /// snapshot end.
    pub last_price: Option<f64>,
    /// Previous session's close. Used by the frontend to compute
    /// change and change-percent.
    pub prev_close: Option<f64>,
    /// Cumulative session volume.
    pub volume: Option<i32>,
    /// Unix epoch seconds when the snapshot completed.
    pub timestamp: i64,
}
