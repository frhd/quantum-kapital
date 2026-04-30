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
    /// Optional IBKR `industryLike` filter. When `Some`, the scan is
    /// constrained to issuers whose `industry` matches the value (e.g.
    /// `"Semiconductors"`). Threaded through to IBKR as a
    /// `scannerSubscriptionFilterOptions` `TagValue`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub industry_filter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerData {
    pub rank: i32,
    pub contract: ContractDetails,
    pub leg: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn industry_filter_round_trips_when_set() {
        let sub = ScannerSubscription {
            number_of_rows: 10,
            instrument: "STK".to_string(),
            location_code: "STK.US.MAJOR".to_string(),
            scan_code: "TOP_PERC_GAIN".to_string(),
            above_price: None,
            below_price: None,
            above_volume: None,
            market_cap_above: None,
            market_cap_below: None,
            industry_filter: Some("Semiconductors".to_string()),
        };
        let json = serde_json::to_string(&sub).unwrap();
        let parsed: ScannerSubscription = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.industry_filter, Some("Semiconductors".to_string()));
    }

    #[test]
    fn industry_filter_defaults_to_none_when_absent_in_json() {
        // Existing config files / frontend payloads omit the field.
        let json = r#"{
            "number_of_rows": 10,
            "instrument": "STK",
            "location_code": "STK.US.MAJOR",
            "scan_code": "TOP_PERC_GAIN"
        }"#;
        let parsed: ScannerSubscription = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.industry_filter, None);
    }
}
