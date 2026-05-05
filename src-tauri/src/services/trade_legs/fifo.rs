//! Pure FIFO leg matcher. No DB, no IBKR. Input: a day's fills for a
//! single account. Output: round-trip + carryover legs.
//!
//! Algorithm: group by contract identity; within each group, queue
//! opens FIFO, consume on closes, emit one leg per closing fill (plus
//! one per leftover open).

use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;

use super::types::{LegTag, LegTotals, TradeLeg};
use crate::ibkr::types::ExecutionSide;
use crate::mcp::tools::executions::ExecutionRow;

#[derive(Debug, Clone)]
struct OpenSlice {
    exec_id: String,
    qty_remaining: f64,
    qty_original: f64,
    price: f64,
    commission: f64, // proportional to qty_remaining/qty_original at time of pop
    opened_at: DateTime<Utc>,
    order_id: i32,
    /// Phase 2 — strategy/setup_id of the opening fill, carried into
    /// the round-trip leg the slice closes.
    strategy: Option<String>,
    setup_id: Option<i64>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct ContractKey {
    symbol: String,
    contract_type: String,
    expiry: Option<NaiveDate>,
    strike_bits: Option<u64>, // f64 bit pattern; reliable hashing
    right: Option<String>,
    multiplier: Option<String>,
}

impl ContractKey {
    fn from(e: &ExecutionRow) -> Self {
        Self {
            symbol: e.symbol.clone(),
            contract_type: e.contract_type.clone(),
            expiry: e.expiry,
            strike_bits: e.strike.map(f64::to_bits),
            right: e.right.clone(),
            multiplier: e.multiplier.clone(),
        }
    }
}

pub fn match_legs(fills: &[ExecutionRow]) -> Vec<TradeLeg> {
    if fills.is_empty() {
        return Vec::new();
    }
    let mut by_key: HashMap<ContractKey, Vec<&ExecutionRow>> = HashMap::new();
    for f in fills {
        by_key.entry(ContractKey::from(f)).or_default().push(f);
    }
    let mut legs: Vec<TradeLeg> = Vec::new();
    let mut leg_counter: usize = 0;
    for (_key, mut group) in by_key {
        group.sort_by_key(|f| f.time);
        // v1: longs only (first fill Bought ⇒ open). Shorts route to a
        // simpler one-leg-per-fill emit tagged complex_strategy.
        let opening_side = match group.first().map(|f| f.side) {
            Some(ExecutionSide::Bought) => ExecutionSide::Bought,
            Some(ExecutionSide::Sold) => {
                emit_short_legs(&group, &mut legs, &mut leg_counter);
                continue;
            }
            None => continue,
        };
        let mut opens: std::collections::VecDeque<OpenSlice> = std::collections::VecDeque::new();
        let mut order_ids_seen: std::collections::HashSet<i32> = std::collections::HashSet::new();
        let mut close_count_in_group = 0usize;
        for f in &group {
            order_ids_seen.insert(f.order_id);
            if f.side == opening_side {
                opens.push_back(OpenSlice {
                    exec_id: f.exec_id.clone(),
                    qty_remaining: f.qty,
                    qty_original: f.qty,
                    price: f.avg_price,
                    commission: f.commission.unwrap_or(0.0),
                    opened_at: f.time,
                    order_id: f.order_id,
                    strategy: f.strategy.clone(),
                    setup_id: f.setup_id,
                });
            } else {
                close_count_in_group += 1;
                let mut close_qty_remaining = f.qty;
                let close_price = f.avg_price;
                let close_commission_total = f.commission.unwrap_or(0.0);
                let close_qty_original = f.qty;
                let mut consumed: Vec<OpenSlice> = Vec::new();
                while close_qty_remaining > 0.0 {
                    let Some(mut front) = opens.pop_front() else {
                        break;
                    };
                    let take = close_qty_remaining.min(front.qty_remaining);
                    let consumed_commission = front.commission * (take / front.qty_original);
                    consumed.push(OpenSlice {
                        exec_id: front.exec_id.clone(),
                        qty_remaining: take,
                        qty_original: front.qty_original,
                        price: front.price,
                        commission: consumed_commission,
                        opened_at: front.opened_at,
                        order_id: front.order_id,
                        strategy: front.strategy.clone(),
                        setup_id: front.setup_id,
                    });
                    front.qty_remaining -= take;
                    front.commission -= consumed_commission;
                    close_qty_remaining -= take;
                    if front.qty_remaining > 1e-9 {
                        opens.push_front(front);
                    }
                }
                // Carry the strategy/setup_id of the first consumed
                // opening slice. Multi-strategy round-trips (rare;
                // requires manually-linked re-entries crossing
                // detector classes) inherit the earliest one — same
                // FIFO logic as the qty consumption.
                let open_strategy = consumed.first().and_then(|o| o.strategy.clone());
                let open_setup_id = consumed.first().and_then(|o| o.setup_id);
                let mut leg = build_round_trip(
                    group[0],
                    f,
                    &consumed,
                    close_price,
                    close_qty_original,
                    close_commission_total,
                    open_strategy,
                    open_setup_id,
                );
                leg.leg_id = next_leg_id(&mut leg_counter);
                legs.push(leg);
            }
        }
        // Carryover: any opens left.
        while let Some(o) = opens.pop_front() {
            let strat = o.strategy.clone();
            let sid = o.setup_id;
            let mut leg = build_carryover(group[0], &o, strat, sid);
            leg.leg_id = next_leg_id(&mut leg_counter);
            legs.push(leg);
        }
        // Tag scaled_out for round-trip legs in groups with >1 close.
        if close_count_in_group > 1 {
            for leg in legs.iter_mut().rev() {
                if leg.symbol == group[0].symbol
                    && leg.tags.contains(&LegTag::RoundTrip)
                    && !leg.tags.contains(&LegTag::ScaledOut)
                {
                    leg.tags.push(LegTag::ScaledOut);
                }
                if !leg.tags.contains(&LegTag::RoundTrip) {
                    break;
                }
            }
        }
        // Heuristic: same order_id across multiple legs in the group ⇒ combo.
        // (rough heuristic; refine if dogfooding finds false positives)
        let _ = order_ids_seen;
    }
    legs.sort_by_key(|l| l.opened_at);
    legs
}

fn emit_short_legs(group: &[&ExecutionRow], legs: &mut Vec<TradeLeg>, counter: &mut usize) {
    for f in group {
        let leg = TradeLeg {
            leg_id: next_leg_id(counter),
            account: f.account.clone(),
            symbol: f.symbol.clone(),
            contract_type: f.contract_type.clone(),
            expiry: f.expiry,
            strike: f.strike,
            right: f.right.clone(),
            multiplier: f.multiplier.clone(),
            opened_at: f.time,
            closed_at: None,
            buy_qty: if matches!(f.side, ExecutionSide::Bought) {
                f.qty
            } else {
                0.0
            },
            avg_buy_price: if matches!(f.side, ExecutionSide::Bought) {
                f.avg_price
            } else {
                0.0
            },
            sell_qty: if matches!(f.side, ExecutionSide::Sold) {
                f.qty
            } else {
                0.0
            },
            avg_sell_price: if matches!(f.side, ExecutionSide::Sold) {
                f.avg_price
            } else {
                0.0
            },
            gross_pnl: 0.0,
            commission_total: f.commission.unwrap_or(0.0),
            net_pnl: -f.commission.unwrap_or(0.0),
            hold_minutes: None,
            source_exec_ids: vec![f.exec_id.clone()],
            tags: vec![LegTag::ComplexStrategy, LegTag::Carryover],
            strategy: f.strategy.clone(),
            setup_id: f.setup_id,
        };
        legs.push(leg);
    }
}

fn next_leg_id(counter: &mut usize) -> String {
    *counter += 1;
    format!("leg_{:03}", counter)
}

#[allow(clippy::too_many_arguments)] // private helper; refactor to a builder if it grows
fn build_round_trip(
    representative: &ExecutionRow,
    close: &ExecutionRow,
    consumed: &[OpenSlice],
    close_price: f64,
    close_qty: f64,
    close_commission_total: f64,
    open_strategy: Option<String>,
    open_setup_id: Option<i64>,
) -> TradeLeg {
    let buy_qty: f64 = consumed.iter().map(|o| o.qty_remaining).sum();
    let buy_notional: f64 = consumed.iter().map(|o| o.qty_remaining * o.price).sum();
    let avg_buy_price = if buy_qty > 0.0 {
        buy_notional / buy_qty
    } else {
        0.0
    };
    let buy_commission: f64 = consumed.iter().map(|o| o.commission).sum();
    let multiplier_factor = representative
        .multiplier
        .as_deref()
        .and_then(|m| m.parse::<f64>().ok())
        .unwrap_or(1.0);
    let sell_qty = close_qty;
    let gross_pnl = (close_price - avg_buy_price) * sell_qty * multiplier_factor;
    let commission_total = buy_commission + close_commission_total;
    let net_pnl = gross_pnl - commission_total;
    let opened_at = consumed.first().map(|o| o.opened_at).unwrap_or(close.time);
    let closed_at = close.time;
    let hold_minutes = (closed_at - opened_at).num_minutes();
    let mut source = consumed
        .iter()
        .map(|o| o.exec_id.clone())
        .collect::<Vec<_>>();
    source.push(close.exec_id.clone());
    let mut tags = vec![LegTag::RoundTrip];
    if consumed.len() > 1 {
        tags.push(LegTag::ScaledIn);
    }
    if consumed
        .iter()
        .any(|o| (o.qty_remaining - o.qty_original).abs() > 1e-9)
    {
        tags.push(LegTag::PartialClose);
    }
    TradeLeg {
        leg_id: String::new(), // assigned by caller
        account: representative.account.clone(),
        symbol: representative.symbol.clone(),
        contract_type: representative.contract_type.clone(),
        expiry: representative.expiry,
        strike: representative.strike,
        right: representative.right.clone(),
        multiplier: representative.multiplier.clone(),
        opened_at,
        closed_at: Some(closed_at),
        buy_qty,
        avg_buy_price,
        sell_qty,
        avg_sell_price: close_price,
        gross_pnl,
        commission_total,
        net_pnl,
        hold_minutes: Some(hold_minutes),
        source_exec_ids: source,
        tags,
        strategy: open_strategy,
        setup_id: open_setup_id,
    }
}

fn build_carryover(
    representative: &ExecutionRow,
    o: &OpenSlice,
    open_strategy: Option<String>,
    open_setup_id: Option<i64>,
) -> TradeLeg {
    TradeLeg {
        leg_id: String::new(),
        account: representative.account.clone(),
        symbol: representative.symbol.clone(),
        contract_type: representative.contract_type.clone(),
        expiry: representative.expiry,
        strike: representative.strike,
        right: representative.right.clone(),
        multiplier: representative.multiplier.clone(),
        opened_at: o.opened_at,
        closed_at: None,
        buy_qty: o.qty_remaining,
        avg_buy_price: o.price,
        sell_qty: 0.0,
        avg_sell_price: 0.0,
        gross_pnl: 0.0,
        commission_total: o.commission,
        net_pnl: -o.commission,
        hold_minutes: None,
        source_exec_ids: vec![o.exec_id.clone()],
        tags: vec![LegTag::Carryover],
        strategy: open_strategy,
        setup_id: open_setup_id,
    }
}

pub fn compute_totals(legs: &[TradeLeg]) -> LegTotals {
    let mut t = LegTotals::default();
    for leg in legs {
        t.gross_pnl += leg.gross_pnl;
        t.net_pnl += leg.net_pnl;
        t.commissions += leg.commission_total;
        if leg.tags.contains(&LegTag::RoundTrip) {
            t.n_round_trips += 1;
        }
        if leg.tags.contains(&LegTag::Carryover) {
            t.n_carryover += 1;
        }
        let entry = t.by_symbol.entry(leg.symbol.clone()).or_default();
        entry.net_pnl += leg.net_pnl;
        entry.n_legs += 1;
    }
    t
}
