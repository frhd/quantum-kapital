//! Tests for the FIFO leg matcher.

use super::fifo::match_legs;
use super::types::LegTag;
use crate::ibkr::types::ExecutionSide;
use crate::mcp::tools::executions::ExecutionRow;
use chrono::{TimeZone, Utc};

fn opt(exec_id: &str, side: ExecutionSide, qty: f64, price: f64, t_min: u32) -> ExecutionRow {
    // t_min may exceed 59 (the plan uses 60, 61 for the last two fills).
    // Decompose into hour offset + remainder so chrono doesn't panic.
    let hour = 17u32 + t_min / 60;
    let min = t_min % 60;
    ExecutionRow {
        exec_id: exec_id.to_string(),
        time: Utc.with_ymd_and_hms(2026, 5, 4, hour, min, 0).unwrap(),
        account: "U1".to_string(),
        symbol: "TSLA".to_string(),
        contract_type: "OPT".to_string(),
        expiry: chrono::NaiveDate::from_ymd_opt(2026, 5, 4),
        strike: Some(395.0),
        right: Some("C".to_string()),
        multiplier: Some("100".to_string()),
        side,
        qty,
        avg_price: price,
        commission: Some(0.50),
        realized_pnl: None,
        currency: Some("USD".to_string()),
        commission_currency: Some("USD".to_string()),
        order_id: 1,
    }
}

#[test]
fn matches_simple_round_trip() {
    let fills = vec![
        opt("OPEN", ExecutionSide::Bought, 3.0, 1.50, 32),
        opt("CLOSE", ExecutionSide::Sold, 3.0, 2.45, 42),
    ];
    let legs = match_legs(&fills);
    assert_eq!(legs.len(), 1, "{:#?}", legs);
    let leg = &legs[0];
    assert_eq!(leg.buy_qty, 3.0);
    assert_eq!(leg.sell_qty, 3.0);
    assert!((leg.avg_buy_price - 1.50).abs() < 1e-9);
    assert!((leg.avg_sell_price - 2.45).abs() < 1e-9);
    // gross = (2.45 - 1.50) * 3 * 100 = 285.00
    assert!((leg.gross_pnl - 285.0).abs() < 1e-6);
    // commissions = 0.50 + 0.50 = 1.00
    assert!((leg.commission_total - 1.00).abs() < 1e-6);
    assert!((leg.net_pnl - 284.0).abs() < 1e-6);
    assert!(leg.tags.contains(&LegTag::RoundTrip));
    assert!(leg.closed_at.is_some());
}

#[test]
fn matches_scaled_in_and_scaled_out() {
    // 4 buys then 5 sells of TSLA 395C, mimicking yesterday's actual trades.
    let fills = vec![
        opt("B1", ExecutionSide::Bought, 3.0, 1.52, 32),
        opt("B2", ExecutionSide::Bought, 1.0, 1.01, 36),
        opt("S1", ExecutionSide::Sold, 2.0, 2.45, 42),
        opt("S2", ExecutionSide::Sold, 2.0, 2.45, 43),
        opt("B3", ExecutionSide::Bought, 1.0, 2.50, 45),
        opt("B4", ExecutionSide::Bought, 1.0, 2.64, 46),
        opt("S3", ExecutionSide::Sold, 2.0, 2.07, 48),
        opt("B5", ExecutionSide::Bought, 2.0, 2.23, 57),
        opt("B6", ExecutionSide::Bought, 1.0, 2.23, 58),
        opt("S4", ExecutionSide::Sold, 1.0, 2.25, 60),
        opt("S5", ExecutionSide::Sold, 2.0, 2.25, 61),
    ];
    let legs = match_legs(&fills);
    // 5 sells ⇒ 5 round-trip legs, 0 carryover (4+1+1+2+1 buys = 9; 2+2+2+1+2 = 9).
    let round = legs
        .iter()
        .filter(|l| l.tags.contains(&LegTag::RoundTrip))
        .count();
    let carry = legs
        .iter()
        .filter(|l| l.tags.contains(&LegTag::Carryover))
        .count();
    assert_eq!(round, 5, "{:#?}", legs);
    assert_eq!(carry, 0);

    // All 5 round-trip legs should be tagged `scaled_out` (group has >1 close).
    for l in &legs {
        if l.tags.contains(&LegTag::RoundTrip) {
            assert!(l.tags.contains(&LegTag::ScaledOut), "leg {:?}", l);
        }
    }
    // Closed positions: buy_qty must equal sell_qty per leg.
    for l in &legs {
        if l.tags.contains(&LegTag::RoundTrip) {
            assert!((l.buy_qty - l.sell_qty).abs() < 1e-9, "leg {:?}", l);
        }
    }
}

#[test]
fn emits_carryover_for_unclosed_open() {
    let fills = vec![opt("OPEN_ONLY", ExecutionSide::Bought, 5.0, 1.00, 32)];
    let legs = match_legs(&fills);
    assert_eq!(legs.len(), 1);
    let leg = &legs[0];
    assert!(leg.tags.contains(&LegTag::Carryover));
    assert!(leg.closed_at.is_none());
    assert_eq!(leg.buy_qty, 5.0);
    assert_eq!(leg.sell_qty, 0.0);
    assert!((leg.gross_pnl - 0.0).abs() < 1e-9);
    // commission should still be charged
    assert!((leg.commission_total - 0.50).abs() < 1e-9);
    assert!((leg.net_pnl - (-0.50)).abs() < 1e-9);
}

#[test]
fn totals_sum_correctly_across_symbols() {
    use super::fifo::compute_totals;
    let fills = vec![
        opt("A1", ExecutionSide::Bought, 1.0, 1.00, 32),
        opt("A2", ExecutionSide::Sold, 1.0, 2.00, 42),
    ];
    let legs = match_legs(&fills);
    let totals = compute_totals(&legs);
    // gross = (2.00 - 1.00) * 1 * 100 = 100.00; commission = 0.50 + 0.50 = 1.00
    assert!((totals.gross_pnl - 100.0).abs() < 1e-9);
    assert!((totals.commissions - 1.0).abs() < 1e-9);
    assert!((totals.net_pnl - 99.0).abs() < 1e-9);
    assert_eq!(totals.n_round_trips, 1);
    assert_eq!(totals.n_carryover, 0);
    assert!(totals.by_symbol.contains_key("TSLA"));
}
