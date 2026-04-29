use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalDataRequest {
    pub symbol: String,
    pub end_date_time: String,
    pub duration: String,
    pub bar_size: BarSize,
    pub what_to_show: WhatToShow,
    pub use_rth: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

impl BarSize {
    /// Stable string used for SQLite storage keys. Compact form, lower-case.
    pub fn as_str(&self) -> &'static str {
        match self {
            BarSize::Sec1 => "1sec",
            BarSize::Sec5 => "5sec",
            BarSize::Sec15 => "15sec",
            BarSize::Sec30 => "30sec",
            BarSize::Min1 => "1min",
            BarSize::Min2 => "2min",
            BarSize::Min3 => "3min",
            BarSize::Min5 => "5min",
            BarSize::Min15 => "15min",
            BarSize::Min20 => "20min",
            BarSize::Min30 => "30min",
            BarSize::Hour1 => "1hour",
            BarSize::Day1 => "1day",
        }
    }

    /// String form used by the IBKR ibapi crate when issuing requests.
    /// (Not exercised by the unit tests — only the live integration uses it.)
    #[allow(dead_code)]
    pub fn ibkr_str(&self) -> &'static str {
        match self {
            BarSize::Sec1 => "1 secs",
            BarSize::Sec5 => "5 secs",
            BarSize::Sec15 => "15 secs",
            BarSize::Sec30 => "30 secs",
            BarSize::Min1 => "1 min",
            BarSize::Min2 => "2 mins",
            BarSize::Min3 => "3 mins",
            BarSize::Min5 => "5 mins",
            BarSize::Min15 => "15 mins",
            BarSize::Min20 => "20 mins",
            BarSize::Min30 => "30 mins",
            BarSize::Hour1 => "1 hour",
            BarSize::Day1 => "1 day",
        }
    }

    /// `true` for sub-day bars; `false` only for `Day1`.
    pub fn is_intraday(&self) -> bool {
        !matches!(self, BarSize::Day1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhatToShow {
    Trades,
    Midpoint,
    Bid,
    Ask,
    BidAsk,
    HistoricalVolatility,
    OptionImpliedVolatility,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Parse an IBKR-formatted bar timestamp into Unix seconds (UTC).
///
/// Accepts `"YYYYMMDD"` (treated as midnight UTC of that day) and
/// `"YYYYMMDD HH:MM:SS"`. Returns a string error on parse failure so
/// the caller can route it through whatever error type they prefer.
pub fn parse_ibkr_time(s: &str) -> Result<i64, String> {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

    let trimmed = s.trim();
    if trimmed.len() == 8 {
        let date = NaiveDate::parse_from_str(trimmed, "%Y%m%d")
            .map_err(|e| format!("invalid IBKR date '{trimmed}': {e}"))?;
        let dt = NaiveDateTime::new(date, NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        return Ok(dt.and_utc().timestamp());
    }

    let dt = NaiveDateTime::parse_from_str(trimmed, "%Y%m%d %H:%M:%S")
        .map_err(|e| format!("invalid IBKR datetime '{trimmed}': {e}"))?;
    Ok(dt.and_utc().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ibkr_time_handles_date_only() {
        let ts = parse_ibkr_time("20240115").expect("parse date");
        // 2024-01-15 00:00:00 UTC
        assert_eq!(ts, 1_705_276_800);
    }

    #[test]
    fn parse_ibkr_time_handles_full_datetime() {
        let ts = parse_ibkr_time("20240115 09:30:00").expect("parse datetime");
        assert_eq!(ts, 1_705_311_000);
    }

    #[test]
    fn parse_ibkr_time_rejects_garbage() {
        assert!(parse_ibkr_time("not a date").is_err());
    }

    #[test]
    fn bar_size_intraday_classification() {
        assert!(BarSize::Min5.is_intraday());
        assert!(BarSize::Hour1.is_intraday());
        assert!(!BarSize::Day1.is_intraday());
    }

    #[test]
    fn bar_size_as_str_is_stable() {
        assert_eq!(BarSize::Day1.as_str(), "1day");
        assert_eq!(BarSize::Min5.as_str(), "5min");
    }
}
