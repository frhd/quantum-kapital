use super::positions::ContractDetails;
use serde::{Deserialize, Serialize};

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
