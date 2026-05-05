//! Build a `LegSummary` from FIFO-matched legs. Pure.
//!
//! Mirrors `agent/trade_review.py::leg_summary_from_legs`. Composes
//! `services::trade_legs::compute_totals` for the shared aggregates and
//! adds a `win_rate` computation (winners / closed, None when no
//! closed legs).

use std::collections::BTreeMap;

use crate::services::trade_legs::{compute_totals, LegTag, TradeLeg};
use crate::services::trade_reviews::types::LegSummary;

pub fn summarize(legs: &[TradeLeg]) -> LegSummary {
    let totals = compute_totals(legs);

    let by_symbol: BTreeMap<String, f64> = totals
        .by_symbol
        .iter()
        .map(|(k, v)| (k.clone(), v.net_pnl))
        .collect();

    let closed: Vec<&TradeLeg> = legs
        .iter()
        .filter(|l| l.tags.contains(&LegTag::RoundTrip))
        .collect();
    let win_rate = if closed.is_empty() {
        None
    } else {
        let winners = closed.iter().filter(|l| l.net_pnl > 0.0).count();
        Some(winners as f64 / closed.len() as f64)
    };

    LegSummary {
        gross_pnl: totals.gross_pnl,
        net_pnl: totals.net_pnl,
        commissions_total: totals.commissions,
        n_round_trips: totals.n_round_trips,
        n_carryover: totals.n_carryover,
        win_rate,
        by_symbol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn leg(leg_id: &str, symbol: &str, gross: f64, comm: f64, net: f64, tag: LegTag) -> TradeLeg {
        TradeLeg {
            leg_id: leg_id.into(),
            account: "U1".into(),
            symbol: symbol.into(),
            contract_type: "STK".into(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            opened_at: chrono::Utc.with_ymd_and_hms(2026, 5, 4, 14, 0, 0).unwrap(),
            closed_at: matches!(tag, LegTag::RoundTrip)
                .then(|| chrono::Utc.with_ymd_and_hms(2026, 5, 4, 15, 0, 0).unwrap()),
            buy_qty: 100.0,
            avg_buy_price: 100.0,
            sell_qty: if matches!(tag, LegTag::RoundTrip) {
                100.0
            } else {
                0.0
            },
            avg_sell_price: if matches!(tag, LegTag::RoundTrip) {
                101.0
            } else {
                0.0
            },
            gross_pnl: gross,
            commission_total: comm,
            net_pnl: net,
            hold_minutes: matches!(tag, LegTag::RoundTrip).then_some(60),
            source_exec_ids: vec![],
            tags: vec![tag],
            strategy: None,
            setup_id: None,
        }
    }

    #[test]
    fn empty_legs_yield_zero_summary_with_none_win_rate() {
        let summary = summarize(&[]);
        assert_eq!(summary.gross_pnl, 0.0);
        assert_eq!(summary.net_pnl, 0.0);
        assert_eq!(summary.commissions_total, 0.0);
        assert_eq!(summary.n_round_trips, 0);
        assert_eq!(summary.n_carryover, 0);
        assert_eq!(summary.win_rate, None);
        assert!(summary.by_symbol.is_empty());
    }

    #[test]
    fn aggregates_round_trip_and_carryover() {
        let legs = vec![
            leg("a", "AAPL", 100.0, 1.0, 99.0, LegTag::RoundTrip),
            leg("b", "AAPL", -50.0, 1.0, -51.0, LegTag::RoundTrip),
            leg("c", "TSLA", 200.0, 2.0, 198.0, LegTag::RoundTrip),
            leg("d", "NVDA", 0.0, 0.0, 0.0, LegTag::Carryover),
        ];
        let s = summarize(&legs);
        assert_eq!(s.gross_pnl, 250.0);
        assert_eq!(s.commissions_total, 4.0);
        assert_eq!(s.net_pnl, 246.0);
        assert_eq!(s.n_round_trips, 3);
        assert_eq!(s.n_carryover, 1);
        // win_rate = 2 winners (a, c) / 3 closed = 0.6666...
        assert!((s.win_rate.unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(*s.by_symbol.get("AAPL").unwrap(), 99.0 + -51.0);
        assert_eq!(*s.by_symbol.get("TSLA").unwrap(), 198.0);
        assert_eq!(*s.by_symbol.get("NVDA").unwrap(), 0.0);
    }

    #[test]
    fn win_rate_is_none_when_no_closed_legs() {
        let legs = vec![leg("c", "NVDA", 0.0, 0.0, 0.0, LegTag::Carryover)];
        let s = summarize(&legs);
        assert_eq!(s.win_rate, None);
        assert_eq!(s.n_round_trips, 0);
        assert_eq!(s.n_carryover, 1);
    }

    #[test]
    fn win_rate_zero_when_all_losers() {
        let legs = vec![
            leg("a", "X", -10.0, 0.5, -10.5, LegTag::RoundTrip),
            leg("b", "Y", -5.0, 0.5, -5.5, LegTag::RoundTrip),
        ];
        let s = summarize(&legs);
        assert_eq!(s.win_rate, Some(0.0));
        assert_eq!(s.net_pnl, -16.0);
    }
}
