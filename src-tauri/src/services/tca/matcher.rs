//! Phase 2 — pure intent ↔ fill matcher.
//!
//! No DB, no IBKR. Input: a list of open `OrderIntent`s and one fill.
//! Output: an optional `LinkageDecision` describing which intent the
//! fill answers and the slippage it incurred.
//!
//! Match rules (mirrored to phase doc):
//! 1. Same `account`, `symbol`, `side` (mapping `Bought`↔`Buy`,
//!    `Sold`↔`Sell`).
//! 2. Fill `time` ∈ `[posted_at, expires_at)` of the intent.
//! 3. Among the survivors, pick the **earliest** `posted_at` that
//!    has remaining qty > 0. Earliest-first preserves FIFO semantics
//!    so a re-entry intent on the same side doesn't poach fills from
//!    an older still-open one.
//! 4. Slippage formula:
//!    - bps = round(|fill_price - intended_price| / intended_price * 10_000)
//!    - signed_cents_per_share:
//!      - long  → round((fill_price - intended_price) * 100)
//!      - short → round((intended_price - fill_price) * 100)
//!
//!    Positive in either direction = trader paid more / received
//!    less than intended.
//!
//! Partial fills: the caller (the store layer) is responsible for
//! taking the matcher's decision and incrementing `matched_qty`. The
//! matcher itself doesn't mutate the intent.

use crate::ibkr::types::{ExecutionSide, IbkrExecution};

use super::types::{IntentSide, IntentStatus, LinkageDecision, OrderIntent, SlippageRecord};

/// Map a past-tense execution side to the buy/sell side an intent
/// was recorded with.
pub fn execution_side_to_intent_side(side: ExecutionSide) -> IntentSide {
    match side {
        ExecutionSide::Bought => IntentSide::Buy,
        ExecutionSide::Sold => IntentSide::Sell,
    }
}

/// Pure slippage math. Pulled out so tests can exercise both signs
/// without constructing an `IbkrExecution`.
pub fn compute_slippage(
    fill_price: f64,
    intended_price_cents: i64,
    side: IntentSide,
) -> SlippageRecord {
    if intended_price_cents <= 0 || !fill_price.is_finite() {
        return SlippageRecord {
            bps: 0,
            signed_cents_per_share: 0,
        };
    }
    let intended = intended_price_cents as f64 / 100.0;
    let diff = fill_price - intended;
    let signed_dollars = match side {
        IntentSide::Buy => diff,
        IntentSide::Sell => -diff,
    };
    // Round-half-away-from-zero so a $0.005 slippage doesn't disappear
    // on the round trip when `i64::round_ties_even` would zero it.
    let signed_cents = (signed_dollars * 100.0).round() as i64;
    let bps = ((diff.abs() / intended) * 10_000.0).round() as i64;
    SlippageRecord {
        bps,
        signed_cents_per_share: signed_cents,
    }
}

/// Try to match `fill` against any of the supplied open `intents`.
/// Returns `Some(LinkageDecision)` when a match is found.
///
/// The caller is expected to have pre-filtered `intents` by
/// `(account, symbol, side, status=Open)` for index efficiency, but
/// the matcher re-checks defensively — it is the only correctness
/// boundary for partial-fill bookkeeping.
pub fn match_fill(fill: &IbkrExecution, intents: &[OrderIntent]) -> Option<LinkageDecision> {
    if intents.is_empty() {
        return None;
    }
    let fill_side = execution_side_to_intent_side(fill.side);
    let mut candidates: Vec<&OrderIntent> = intents
        .iter()
        .filter(|i| i.status == IntentStatus::Open)
        .filter(|i| i.account == fill.account)
        .filter(|i| i.symbol == fill.symbol)
        .filter(|i| i.side == fill_side)
        .filter(|i| fill.exec_time >= i.posted_at && fill.exec_time < i.expires_at)
        .filter(|i| i.remaining_qty() > 1e-9)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    // FIFO across same-shape intents.
    candidates.sort_by_key(|i| i.posted_at);
    let intent = candidates[0];
    let slip = compute_slippage(fill.avg_price, intent.intended_price_cents, intent.side);
    let new_matched_qty = intent.matched_qty + fill.qty;
    Some(LinkageDecision {
        exec_id: fill.exec_id.clone(),
        intent_id: intent.intent_id.clone(),
        setup_id: intent.setup_id,
        intended_price_cents: intent.intended_price_cents,
        intended_price_source: intent.intended_price_source,
        slippage_bps: slip.bps,
        slippage_signed: slip.signed_cents_per_share,
        new_matched_qty,
    })
}

#[cfg(test)]
mod tests {
    use super::super::types::IntendedPriceSource;
    use super::*;
    use chrono::{Duration, Utc};

    fn intent(side: IntentSide, qty: f64, price_cents: i64, age_minutes: i64) -> OrderIntent {
        let posted = Utc::now() - Duration::minutes(age_minutes);
        OrderIntent {
            intent_id: format!("i_{}_{}", side.as_str(), age_minutes),
            setup_id: Some(42),
            account: "DU1".to_string(),
            symbol: "AAPL".to_string(),
            side,
            qty,
            intended_price_cents: price_cents,
            intended_price_source: IntendedPriceSource::TriggerPrice,
            posted_at: posted,
            expires_at: posted + Duration::minutes(60),
            status: IntentStatus::Open,
            matched_qty: 0.0,
        }
    }

    fn fill(symbol: &str, side: ExecutionSide, qty: f64, price: f64) -> IbkrExecution {
        IbkrExecution {
            symbol: symbol.to_string(),
            side,
            qty,
            avg_price: price,
            exec_time: Utc::now(),
            order_id: 1,
            exec_id: format!("e_{}_{}", symbol, qty as u32),
            account: "DU1".to_string(),
            contract_type: "STK".to_string(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            commission: None,
            realized_pnl: None,
            currency: Some("USD".to_string()),
            commission_currency: None,
        }
    }

    #[test]
    fn long_paying_more_than_intended_is_positive_slippage() {
        let s = compute_slippage(101.50, 10_000, IntentSide::Buy);
        // 1.5% = 150 bps; +$1.50 = +150 cents/share.
        assert_eq!(s.bps, 150);
        assert_eq!(s.signed_cents_per_share, 150);
    }

    #[test]
    fn short_receiving_less_than_intended_is_positive_slippage() {
        // Intended sell @ $100.00, filled @ $98.50 ⇒ trader received less.
        let s = compute_slippage(98.50, 10_000, IntentSide::Sell);
        assert_eq!(s.bps, 150);
        assert_eq!(s.signed_cents_per_share, 150);
    }

    #[test]
    fn long_paying_less_than_intended_is_negative_signed_slippage() {
        // Trader got a better fill than intended.
        let s = compute_slippage(99.50, 10_000, IntentSide::Buy);
        assert_eq!(s.bps, 50);
        assert_eq!(s.signed_cents_per_share, -50);
    }

    #[test]
    fn slippage_zero_at_perfect_fill() {
        let s = compute_slippage(100.00, 10_000, IntentSide::Buy);
        assert_eq!(s.bps, 0);
        assert_eq!(s.signed_cents_per_share, 0);
    }

    #[test]
    fn slippage_bails_on_zero_intended() {
        let s = compute_slippage(100.00, 0, IntentSide::Buy);
        assert_eq!(s.bps, 0);
        assert_eq!(s.signed_cents_per_share, 0);
    }

    #[test]
    fn match_clean_fill_picks_only_open_intent() {
        let i = intent(IntentSide::Buy, 100.0, 10_000, 1);
        let f = fill("AAPL", ExecutionSide::Bought, 100.0, 100.50);
        let d = match_fill(&f, std::slice::from_ref(&i)).unwrap();
        assert_eq!(d.intent_id, i.intent_id);
        assert_eq!(d.slippage_bps, 50);
        assert_eq!(d.new_matched_qty, 100.0);
    }

    #[test]
    fn match_fifo_picks_earliest_open_intent() {
        let older = intent(IntentSide::Buy, 100.0, 10_000, 30);
        let newer = intent(IntentSide::Buy, 100.0, 10_000, 1);
        let f = fill("AAPL", ExecutionSide::Bought, 100.0, 100.00);
        let d = match_fill(&f, &[newer, older.clone()]).unwrap();
        assert_eq!(d.intent_id, older.intent_id);
    }

    #[test]
    fn match_skips_expired_window() {
        let mut i = intent(IntentSide::Buy, 100.0, 10_000, 90);
        // Posted 90m ago, default window 60m ⇒ expired before now.
        i.expires_at = i.posted_at + Duration::minutes(60);
        let f = fill("AAPL", ExecutionSide::Bought, 100.0, 100.0);
        assert!(match_fill(&f, &[i]).is_none());
    }

    #[test]
    fn match_skips_wrong_side() {
        let i = intent(IntentSide::Sell, 100.0, 10_000, 1);
        let f = fill("AAPL", ExecutionSide::Bought, 100.0, 100.0);
        assert!(match_fill(&f, &[i]).is_none());
    }

    #[test]
    fn match_skips_wrong_symbol() {
        let i = intent(IntentSide::Buy, 100.0, 10_000, 1);
        let f = fill("MSFT", ExecutionSide::Bought, 100.0, 100.0);
        assert!(match_fill(&f, &[i]).is_none());
    }

    #[test]
    fn match_skips_filled_intent() {
        let mut i = intent(IntentSide::Buy, 100.0, 10_000, 1);
        i.matched_qty = 100.0;
        let f = fill("AAPL", ExecutionSide::Bought, 100.0, 100.0);
        assert!(match_fill(&f, &[i]).is_none());
    }

    #[test]
    fn match_partial_carries_cumulative_matched_qty() {
        let mut i = intent(IntentSide::Buy, 100.0, 10_000, 1);
        i.matched_qty = 60.0;
        let f = fill("AAPL", ExecutionSide::Bought, 40.0, 100.25);
        let d = match_fill(&f, &[i]).unwrap();
        assert_eq!(d.new_matched_qty, 100.0);
        assert_eq!(d.slippage_bps, 25);
    }

    #[test]
    fn match_skips_non_open_status() {
        let mut i = intent(IntentSide::Buy, 100.0, 10_000, 1);
        i.status = IntentStatus::Expired;
        let f = fill("AAPL", ExecutionSide::Bought, 100.0, 100.0);
        assert!(match_fill(&f, &[i]).is_none());
    }
}
