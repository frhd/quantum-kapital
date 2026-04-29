//! Pricing table for supported Anthropic models. All values are USD per million tokens.
//! Update at implementation time with current pricing — values below match the 2025-11 pricing.

/// Returns (input_per_million, output_per_million, cache_read_per_million) or None for unknown models.
pub fn price_for(model: &str) -> Option<(f64, f64, f64)> {
    match model {
        "claude-sonnet-4-6" => Some((3.0, 15.0, 0.30)),
        "claude-haiku-4-5" => Some((1.0, 5.0, 0.10)),
        _ => None,
    }
}

/// Compute USD cost for one message. Returns None when the model is unknown.
pub fn cost_usd(
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
) -> Option<f64> {
    let (input_rate, output_rate, cache_rate) = price_for(model)?;
    let cost = (input_tokens as f64 * input_rate
        + output_tokens as f64 * output_rate
        + cache_read_tokens as f64 * cache_rate)
        / 1_000_000.0;
    Some(cost)
}
