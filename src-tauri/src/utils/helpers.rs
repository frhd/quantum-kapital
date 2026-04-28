/// Get current timestamp in milliseconds
pub fn current_timestamp_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Format currency value with 2 decimal places
pub fn format_currency(value: f64, currency: &str) -> String {
    format!("{currency} {value:.2}")
}

/// Validate that a string is a valid symbol
pub fn is_valid_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol.len() <= 12
        && symbol
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
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
