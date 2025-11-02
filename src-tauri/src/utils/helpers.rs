#![allow(dead_code)]

use chrono::{DateTime, Local, TimeZone, Utc};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// Get current timestamp in milliseconds
pub fn current_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Format timestamp to human readable string
pub fn format_timestamp(timestamp_ms: i64) -> String {
    let datetime = Utc.timestamp_millis_opt(timestamp_ms).unwrap();
    datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Convert UTC timestamp to local time
pub fn to_local_time(timestamp_ms: i64) -> DateTime<Local> {
    let utc = Utc.timestamp_millis_opt(timestamp_ms).unwrap();
    utc.with_timezone(&Local)
}

/// Round a float to specified decimal places
pub fn round_to_decimals(value: f64, decimals: u32) -> f64 {
    let multiplier = 10f64.powi(decimals as i32);
    (value * multiplier).round() / multiplier
}

/// Format currency value with 2 decimal places
pub fn format_currency(value: f64, currency: &str) -> String {
    format!("{currency} {value:.2}")
}

/// Format percentage with 2 decimal places
pub fn format_percentage(value: f64) -> String {
    format!("{value:.2}%")
}

/// Parse a JSON value to extract a specific field
pub fn extract_json_field<T: serde::de::DeserializeOwned>(
    json: &Value,
    field_path: &str,
) -> Option<T> {
    let parts: Vec<&str> = field_path.split('.').collect();
    let mut current = json;

    for part in parts {
        current = current.get(part)?;
    }

    serde_json::from_value(current.clone()).ok()
}

/// Validate that a string is a valid symbol
pub fn is_valid_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol.len() <= 12
        && symbol
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
}

/// Sanitize a string for logging (remove sensitive data)
pub fn sanitize_for_logging(input: &str) -> String {
    // Replace potential account numbers or sensitive data
    let mut result = input.to_string();

    // Replace account numbers (assuming they're alphanumeric and 5-15 chars)
    let account_regex = regex::Regex::new(r"\b[A-Za-z0-9]{5,15}\b").unwrap();
    result = account_regex.replace_all(&result, "[ACCOUNT]").to_string();

    // Replace potential API keys or tokens
    let token_regex = regex::Regex::new(r"\b[A-Za-z0-9_-]{20,}\b").unwrap();
    result = token_regex.replace_all(&result, "[TOKEN]").to_string();

    result
}

/// Calculate percentage change
pub fn calculate_percentage_change(old_value: f64, new_value: f64) -> f64 {
    if old_value == 0.0 {
        return 0.0;
    }
    ((new_value - old_value) / old_value.abs()) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_to_decimals() {
        assert_eq!(round_to_decimals(1.236789, 2), 1.24);
        assert_eq!(round_to_decimals(2.71559, 2), 2.72);
        assert_eq!(round_to_decimals(1.234567, 4), 1.2346);
        assert_eq!(round_to_decimals(5.678901, 3), 5.679);
    }

    #[test]
    fn test_format_currency() {
        assert_eq!(format_currency(1234.56, "USD"), "USD 1234.56");
        assert_eq!(format_currency(1234.5, "EUR"), "EUR 1234.50");
    }

    #[test]
    fn test_is_valid_symbol() {
        assert!(is_valid_symbol("AAPL"));
        assert!(is_valid_symbol("BRK.B"));
        assert!(!is_valid_symbol(""));
        assert!(!is_valid_symbol("VERYLONGSYMBOL"));
        assert!(!is_valid_symbol("AAPL$"));
    }

    #[test]
    fn test_calculate_percentage_change() {
        assert_eq!(calculate_percentage_change(100.0, 110.0), 10.0);
        assert_eq!(calculate_percentage_change(100.0, 90.0), -10.0);
        assert_eq!(calculate_percentage_change(0.0, 100.0), 0.0);
    }
}
