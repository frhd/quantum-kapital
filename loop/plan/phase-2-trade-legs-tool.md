# Phase 2 — `get_trade_legs` MCP aggregator (FIFO leg-matching)

> Part of [Behavioral assessment via MCP](master.md). See master for invariants.

**Status:** todo

**Depends on:** 1 (executions store must exist so multi-day queries work).

**Goal:** Ship `get_trade_legs(date, account?, symbol?)` MCP read tool. It groups raw fills into round-trip legs (matched buys + sells) and carryover legs (unclosed positions) via FIFO matching within a `(account, symbol, contract_type, expiry, strike, right)` key. Returns leg-level realized P&L net of commissions, plus a `totals` rollup. Replaces the manual leg-matching the LLM had to do by hand to produce yesterday's review.

**Why this matters:** the "rate yesterday's trades" assessment requires leg-level P&L. Without this tool, every consumer (the LLM client today, the extended `eod_review.py` in Phase 4) re-implements FIFO matching ad-hoc. With it, leg-level math is computed once, cached on read, and matches IBKR Trade Log to the cent.

## End-state for this phase

- `services/trade_legs/` module exists with a pure FIFO matcher.
- `mcp/tools/get_trade_legs.rs` registers the new MCP tool, mirroring `executions.rs`'s shape.
- One MCP call returns a structured response:
  ```jsonc
  {
    "date": "2026-05-04",
    "account": "U4393159",
    "legs": [
      {
        "leg_id": "leg_001",
        "symbol": "TSLA", "contract_type": "OPT",
        "expiry": "2026-05-04", "strike": 395.0, "right": "C", "multiplier": "100",
        "opened_at": "2026-05-04T17:32:03Z",
        "closed_at": "2026-05-04T18:03:30Z",
        "buy_qty": 9.0, "avg_buy_price": 1.92,
        "sell_qty": 9.0, "avg_sell_price": 2.31,
        "gross_pnl": 351.00,
        "commission_total": 13.79,
        "net_pnl": 337.21,
        "hold_minutes": 31,
        "source_exec_ids": ["000243ef.69f88564.01.01", "..."],
        "tags": ["round_trip", "scaled_in", "scaled_out"]
      },
      // ...possibly carryover legs with closed_at = null...
    ],
    "totals": {
      "gross_pnl": 668.78,
      "net_pnl": 401.10,
      "commissions": 20.92,
      "n_round_trips": 3,
      "n_carryover": 0,
      "by_symbol": { "TSLA": { "net_pnl": 401.10, "n_legs": 3 } }
    }
  }
  ```

## Files

**Create:**
- `src-tauri/src/services/trade_legs/mod.rs` — module root.
- `src-tauri/src/services/trade_legs/fifo.rs` — pure FIFO matcher (no DB, no IBKR). Takes `&[IbkrExecution]`, returns `Vec<TradeLeg>`.
- `src-tauri/src/services/trade_legs/types.rs` — `TradeLeg`, `LegTag`, `LegTotals`, `LegSummary` DTOs.
- `src-tauri/src/services/trade_legs/tests.rs` — unit tests for the matcher.
- `src-tauri/src/mcp/tools/get_trade_legs.rs` — MCP tool, mirrors `executions.rs` (read-only, no audit, account resolution).

**Modify:**
- `src-tauri/src/services/mod.rs` — add `pub mod trade_legs;`.
- `src-tauri/src/mcp/tools/mod.rs` — register the new tool router.
- `src-tauri/src/mcp/handler.rs` — wire the new tool router (mirror `executions_router`'s registration).

## Reuse

- `IbkrExecution` shape (already populated by Phase 1's store) — input to the matcher.
- `AccountReader::executions(account, date)` (extended in Phase 1) — fetches the input rows, transparently serving from store or live IBKR.
- `mcp::tools::resolve_account(...)` — account-arg resolution, identical to `executions.rs`.
- `mcp::tools::map_tool_result(...)` — error/ok wrapping.
- `mcp::tools::test_support::handler_for_mock_ibkr` — the test seam; mirror the existing `executions.rs` tests.

## End-state types

```rust
// services/trade_legs/types.rs
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LegTag {
    RoundTrip,
    Carryover,
    ScaledIn,
    ScaledOut,
    PartialClose,
    ComplexStrategy,    // multi-leg combo heuristic (same order_id across legs)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeLeg {
    pub leg_id: String,
    pub account: String,
    pub symbol: String,
    pub contract_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry: Option<NaiveDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<String>,
    pub opened_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
    pub buy_qty: f64,
    pub avg_buy_price: f64,
    pub sell_qty: f64,
    pub avg_sell_price: f64,
    /// Realised P&L gross of commissions. For carryover legs: 0.0.
    pub gross_pnl: f64,
    pub commission_total: f64,
    pub net_pnl: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold_minutes: Option<i64>,
    pub source_exec_ids: Vec<String>,
    pub tags: Vec<LegTag>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LegTotals {
    pub gross_pnl: f64,
    pub net_pnl: f64,
    pub commissions: f64,
    pub n_round_trips: usize,
    pub n_carryover: usize,
    pub by_symbol: std::collections::BTreeMap<String, SymbolTotals>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SymbolTotals {
    pub net_pnl: f64,
    pub n_legs: usize,
}
```

## FIFO algorithm spec

Input: `Vec<IbkrExecution>` for a single (account, date), in any order.

Steps:
1. Group fills by `(symbol, contract_type, expiry, strike, right, multiplier)` — the **contract identity tuple**.
2. Within each group, sort fills by `exec_time` ascending.
3. Maintain two FIFO queues per group: `opens` (the opening side) and `closes` (the closing side).
   - Determine "opening side" by the first fill's `side` (Bought ⇒ longs, Sold ⇒ shorts). v1 only handles longs (Bought = open, Sold = close); shorts get tagged `complex_strategy` and emit one leg per fill.
4. For each fill in time order:
   - If it's an opening fill (Bought, for longs): push onto `opens`.
   - If it's a closing fill (Sold, for longs): pop from the front of `opens`, partially-or-fully consuming each open until the close's qty is exhausted. Each partial consumption produces one **closing match record**.
5. After all fills are processed:
   - Each closing match record contributes to a **round-trip leg** (one round-trip leg per closing fill, summarising all opens it consumed).
   - Anything left in `opens` becomes a **carryover leg** (one leg per remaining open fill, with `closed_at = None` and `gross_pnl = 0`).
6. Per leg, compute:
   - `buy_qty`, `avg_buy_price` (qty-weighted) over the consumed open portions.
   - `sell_qty`, `avg_sell_price` (qty-weighted) over the close fill(s) that contributed.
   - `commission_total = sum(commissions across all source exec_ids)` (each fill's commission proportionally allocated when partially consumed).
   - `gross_pnl = (avg_sell_price - avg_buy_price) * sell_qty * multiplier_factor` where `multiplier_factor = multiplier.parse::<f64>().unwrap_or(1.0)` for OPT, 1.0 for STK.
   - `net_pnl = gross_pnl - commission_total`.
   - `hold_minutes = (closed_at - opened_at).num_minutes()` for round-trips, `None` for carryover.
7. Tags:
   - `round_trip` — closed leg.
   - `carryover` — unclosed leg.
   - `scaled_in` — leg consumed >1 open fill.
   - `scaled_out` — symbol's `(date, contract_identity)` group has >1 closing fill.
   - `partial_close` — close consumed only part of one open (the open will appear in another leg's source_exec_ids).
   - `complex_strategy` — multiple legs share the same `order_id` (heuristic for a combo order).

> **v1 short-side handling.** Shorts (open with Sold, close with Bought) are uncommon in this account but not impossible. v1 emits one leg per Sell-then-Buy pair using the same FIFO logic with sides flipped, tagged `complex_strategy`. Detect by "first fill in group is Sold" and route to the symmetric matcher. v2 may invest more in shorts when dogfooding shows them.

## Tasks

### Task 1: Types module + skeleton

**Files:**
- Create: `src-tauri/src/services/trade_legs/types.rs`
- Create: `src-tauri/src/services/trade_legs/mod.rs`
- Modify: `src-tauri/src/services/mod.rs`

- [ ] **Step 1: Write the types module** (paste the `types.rs` block above).

- [ ] **Step 2: Write `mod.rs`**

```rust
//! `trade_legs` — FIFO leg matcher over a day of fills.

pub mod fifo;
pub mod types;

pub use fifo::match_legs;
pub use types::{LegTag, LegTotals, SymbolTotals, TradeLeg};

#[cfg(test)]
mod tests;
```

- [ ] **Step 3: Wire into the parent module**

In `src-tauri/src/services/mod.rs`, add `pub mod trade_legs;` next to the other modules.

- [ ] **Step 4: cargo check**

```bash
cd src-tauri && cargo check
```

Expected: compiler complains `fifo.rs` doesn't exist. Move on to Task 2.

### Task 2: Failing test for a simple round-trip

**Files:**
- Create: `src-tauri/src/services/trade_legs/tests.rs`

- [ ] **Step 1: Write the test**

```rust
//! Tests for the FIFO leg matcher.

use super::fifo::match_legs;
use super::types::LegTag;
use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use chrono::{TimeZone, Utc};

fn opt(exec_id: &str, side: ExecutionSide, qty: f64, price: f64, t_min: u32) -> IbkrExecution {
    IbkrExecution {
        exec_id: exec_id.to_string(),
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
        currency: Some("USD".to_string()),
        exec_time: Utc.with_ymd_and_hms(2026, 5, 4, 17, t_min, 0).unwrap(),
        order_id: 1,
        commission: Some(0.50),
        realized_pnl: None,
        commission_currency: Some("USD".to_string()),
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
```

- [ ] **Step 2: Run, verify it fails**

```bash
cd src-tauri && cargo test services::trade_legs::tests::matches_simple_round_trip 2>&1 | tail -10
```

Expected: FAIL — `match_legs` doesn't exist yet.

### Task 3: Implement `match_legs` — minimal pass for the simple case

**Files:**
- Create: `src-tauri/src/services/trade_legs/fifo.rs`

- [ ] **Step 1: Write the matcher**

```rust
//! Pure FIFO leg matcher. No DB, no IBKR. Input: a day's fills for a
//! single account. Output: round-trip + carryover legs.
//!
//! Algorithm: group by contract identity; within each group, queue
//! opens FIFO, consume on closes, emit one leg per closing fill (plus
//! one per leftover open).

use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;

use crate::ibkr::types::{ExecutionSide, IbkrExecution};
use super::types::{LegTag, LegTotals, SymbolTotals, TradeLeg};

#[derive(Debug, Clone)]
struct OpenSlice {
    exec_id: String,
    qty_remaining: f64,
    qty_original: f64,
    price: f64,
    commission: f64, // proportional to qty_remaining/qty_original at time of pop
    opened_at: DateTime<Utc>,
    order_id: i32,
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
    fn from(e: &IbkrExecution) -> Self {
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

pub fn match_legs(fills: &[IbkrExecution]) -> Vec<TradeLeg> {
    if fills.is_empty() {
        return Vec::new();
    }
    let mut by_key: HashMap<ContractKey, Vec<&IbkrExecution>> = HashMap::new();
    for f in fills {
        by_key.entry(ContractKey::from(f)).or_default().push(f);
    }
    let mut legs: Vec<TradeLeg> = Vec::new();
    let mut leg_counter: usize = 0;
    for (_key, mut group) in by_key {
        group.sort_by_key(|f| f.exec_time);
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
                    opened_at: f.exec_time,
                    order_id: f.order_id,
                });
            } else {
                close_count_in_group += 1;
                let mut close_qty_remaining = f.qty;
                let close_price = f.avg_price;
                let close_commission_total = f.commission.unwrap_or(0.0);
                let close_qty_original = f.qty;
                let mut consumed: Vec<OpenSlice> = Vec::new();
                while close_qty_remaining > 0.0 {
                    let Some(mut front) = opens.pop_front() else { break };
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
                    });
                    front.qty_remaining -= take;
                    front.commission -= consumed_commission;
                    close_qty_remaining -= take;
                    if front.qty_remaining > 1e-9 {
                        opens.push_front(front);
                    }
                }
                let mut leg = build_round_trip(
                    &group[0],
                    f,
                    &consumed,
                    close_price,
                    close_qty_original,
                    close_commission_total,
                );
                leg.leg_id = next_leg_id(&mut leg_counter);
                legs.push(leg);
            }
        }
        // Carryover: any opens left.
        while let Some(o) = opens.pop_front() {
            let mut leg = build_carryover(&group[0], &o);
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
        if order_ids_seen.len() < group.len() && group.len() > 1 {
            // (rough heuristic; refine if dogfooding finds false positives)
        }
    }
    legs.sort_by_key(|l| l.opened_at);
    legs
}

fn emit_short_legs(
    group: &[&IbkrExecution],
    legs: &mut Vec<TradeLeg>,
    counter: &mut usize,
) {
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
            opened_at: f.exec_time,
            closed_at: None,
            buy_qty: if matches!(f.side, ExecutionSide::Bought) { f.qty } else { 0.0 },
            avg_buy_price: if matches!(f.side, ExecutionSide::Bought) { f.avg_price } else { 0.0 },
            sell_qty: if matches!(f.side, ExecutionSide::Sold) { f.qty } else { 0.0 },
            avg_sell_price: if matches!(f.side, ExecutionSide::Sold) { f.avg_price } else { 0.0 },
            gross_pnl: 0.0,
            commission_total: f.commission.unwrap_or(0.0),
            net_pnl: -f.commission.unwrap_or(0.0),
            hold_minutes: None,
            source_exec_ids: vec![f.exec_id.clone()],
            tags: vec![LegTag::ComplexStrategy, LegTag::Carryover],
        };
        legs.push(leg);
    }
}

fn next_leg_id(counter: &mut usize) -> String {
    *counter += 1;
    format!("leg_{:03}", counter)
}

fn build_round_trip(
    representative: &IbkrExecution,
    close: &IbkrExecution,
    consumed: &[OpenSlice],
    close_price: f64,
    close_qty: f64,
    close_commission_total: f64,
) -> TradeLeg {
    let buy_qty: f64 = consumed.iter().map(|o| o.qty_remaining).sum();
    let buy_notional: f64 = consumed.iter().map(|o| o.qty_remaining * o.price).sum();
    let avg_buy_price = if buy_qty > 0.0 { buy_notional / buy_qty } else { 0.0 };
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
    let opened_at = consumed.first().map(|o| o.opened_at).unwrap_or(close.exec_time);
    let closed_at = close.exec_time;
    let hold_minutes = (closed_at - opened_at).num_minutes();
    let mut source = consumed.iter().map(|o| o.exec_id.clone()).collect::<Vec<_>>();
    source.push(close.exec_id.clone());
    let mut tags = vec![LegTag::RoundTrip];
    if consumed.len() > 1 {
        tags.push(LegTag::ScaledIn);
    }
    if consumed.iter().any(|o| (o.qty_remaining - o.qty_original).abs() > 1e-9) {
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
    }
}

fn build_carryover(representative: &IbkrExecution, o: &OpenSlice) -> TradeLeg {
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
```

- [ ] **Step 2: Run, verify it passes**

```bash
cd src-tauri && cargo test services::trade_legs::tests::matches_simple_round_trip
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/trade_legs/ src-tauri/src/services/mod.rs
git commit -m "feat(trade_legs): FIFO matcher with simple round-trip support"
```

### Task 4: Scaled-in / scaled-out

**Files:**
- Modify: `src-tauri/src/services/trade_legs/tests.rs`

- [ ] **Step 1: Write the test mirroring yesterday's TSLA $395C trade**

```rust
#[test]
fn matches_scaled_in_and_scaled_out() {
    // 4 buys then 5 sells of TSLA 395C, mimicking yesterday's actual trades.
    let fills = vec![
        opt("B1", ExecutionSide::Bought, 3.0, 1.52, 32),
        opt("B2", ExecutionSide::Bought, 1.0, 1.01, 36),
        opt("S1", ExecutionSide::Sold,   2.0, 2.45, 42),
        opt("S2", ExecutionSide::Sold,   2.0, 2.45, 43),
        opt("B3", ExecutionSide::Bought, 1.0, 2.50, 45),
        opt("B4", ExecutionSide::Bought, 1.0, 2.64, 46),
        opt("S3", ExecutionSide::Sold,   2.0, 2.07, 48),
        opt("B5", ExecutionSide::Bought, 2.0, 2.23, 57),
        opt("B6", ExecutionSide::Bought, 1.0, 2.23, 58),
        opt("S4", ExecutionSide::Sold,   1.0, 2.25, 60),
        opt("S5", ExecutionSide::Sold,   2.0, 2.25, 61),
    ];
    let legs = match_legs(&fills);
    // 5 sells ⇒ 5 round-trip legs, 0 carryover (4+1+1+2+1 buys = 9; 2+2+2+1+2 = 9).
    let round = legs.iter().filter(|l| l.tags.contains(&LegTag::RoundTrip)).count();
    let carry = legs.iter().filter(|l| l.tags.contains(&LegTag::Carryover)).count();
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
```

- [ ] **Step 2: Run, verify it passes** (should pass against the Task 3 implementation; if not, adjust the matcher)

```bash
cd src-tauri && cargo test services::trade_legs::tests::matches_scaled_in_and_scaled_out
```

Expected: PASS.

### Task 5: Carryover when no closes

**Files:**
- Modify: `src-tauri/src/services/trade_legs/tests.rs`

- [ ] **Step 1: Write the test**

```rust
#[test]
fn emits_carryover_for_unclosed_open() {
    let fills = vec![
        opt("OPEN_ONLY", ExecutionSide::Bought, 5.0, 1.00, 32),
    ];
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
```

- [ ] **Step 2: Run, verify it passes**

```bash
cd src-tauri && cargo test services::trade_legs::tests::emits_carryover_for_unclosed_open
```

Expected: PASS.

### Task 6: Totals math

**Files:**
- Modify: `src-tauri/src/services/trade_legs/tests.rs`

- [ ] **Step 1: Write the test**

```rust
#[test]
fn totals_sum_correctly_across_symbols() {
    use super::fifo::compute_totals;
    let fills = vec![
        opt("A1", ExecutionSide::Bought, 1.0, 1.00, 32),
        opt("A2", ExecutionSide::Sold,   1.0, 2.00, 42),
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
```

- [ ] **Step 2: Run, verify it passes**

```bash
cd src-tauri && cargo test services::trade_legs::tests::totals_sum_correctly_across_symbols
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/trade_legs/tests.rs
git commit -m "test(trade_legs): scaled in/out, carryover, totals coverage"
```

### Task 7: MCP tool registration

**Files:**
- Create: `src-tauri/src/mcp/tools/get_trade_legs.rs`
- Modify: `src-tauri/src/mcp/tools/mod.rs` — add `pub mod get_trade_legs;`
- Modify: `src-tauri/src/mcp/handler.rs` — register `get_trade_legs_router`.

- [ ] **Step 1: Write the failing tool test**

```rust
// inside get_trade_legs.rs::tests
#[tokio::test]
async fn get_trade_legs_returns_round_trip_legs() {
    let (_tmp, db) = make_db();
    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["U1".to_string()]).await;
    let t = |min: u32| Utc.with_ymd_and_hms(2026, 5, 4, 17, min, 0).unwrap();
    mock.set_executions(vec![
        opt_exec("OPEN", ExecutionSide::Bought, 3.0, 1.50, t(32)),
        opt_exec("CLOSE", ExecutionSide::Sold,  3.0, 2.45, t(42)),
    ])
    .await;
    let handler = handler_for_mock_ibkr(db, mock).await;

    let result = handler
        .get_trade_legs(Parameters(GetTradeLegsArgs {
            account: Some("U1".into()),
            date: Some("2026-05-04".into()),
            symbol: None,
        }))
        .await
        .expect("ok");
    assert_eq!(result.is_error, Some(false), "{:?}", result);
    let body = result.structured_content.expect("structured");
    let legs = body["legs"].as_array().expect("legs array");
    assert_eq!(legs.len(), 1);
    assert_eq!(legs[0]["symbol"], "TSLA");
    let totals = &body["totals"];
    assert_eq!(totals["n_round_trips"].as_u64().unwrap(), 1);
    assert!((totals["gross_pnl"].as_f64().unwrap() - 285.0).abs() < 1e-6);
}
```

(Top-of-file imports + helpers identical in shape to `executions.rs::tests`.)

- [ ] **Step 2: Implement the tool**

```rust
//! `get_trade_legs` — FIFO leg-matched view of fills for a date.
//!
//! Calls `AccountReader::executions` (transparently served from the
//! Phase 1 store for past days, live IBKR for today), then runs the
//! pure FIFO matcher to produce round-trip + carryover legs with
//! per-leg net P&L. Replaces the manual leg-grouping arithmetic
//! historical LLM clients had to do by hand.

use chrono::{NaiveDate, Utc};
use chrono_tz::America::New_York;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{model::CallToolResult, tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::mcp::handler::McpHandler;
use crate::mcp::tools::{map_tool_result, resolve_account};
use crate::services::trade_legs::{compute_totals, match_legs};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetTradeLegsArgs {
    /// IBKR account ID. Optional — defaults to the sole managed account.
    #[serde(default)]
    pub account: Option<String>,
    /// ISO 8601 ET trading day. Optional — defaults to today.
    #[serde(default)]
    pub date: Option<String>,
    /// Optional symbol filter (case-insensitive).
    #[serde(default)]
    pub symbol: Option<String>,
}

#[tool_router(router = get_trade_legs_router, vis = "pub(crate)")]
impl McpHandler {
    #[tool(
        name = "get_trade_legs",
        description = "FIFO leg-matched view of fills for `date` (defaults to today, ET trading day). Groups buys+sells per (symbol, contract_type, expiry, strike, right) into round-trip legs with realized P&L net of commissions; emits one carryover leg per unclosed open. Returns `{ date, account, legs: [TradeLeg, ...], totals: { gross_pnl, net_pnl, commissions, n_round_trips, n_carryover, by_symbol } }`. Past dates are served from the executions store; current day is fresh from IBKR. Errors if the IBKR connection is down for today's date."
    )]
    pub async fn get_trade_legs(
        &self,
        Parameters(args): Parameters<GetTradeLegsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let date = match args.date.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                Ok(d) => d,
                Err(e) => return map_tool_result::<(), String>(Err(format!("invalid date: {e}"))),
            },
            None => Utc::now().with_timezone(&New_York).date_naive(),
        };
        let account = match resolve_account(self.ibkr_client.as_ref(), args.account.as_deref()).await {
            Ok(a) => a,
            Err(e) => return map_tool_result::<(), String>(Err(e)),
        };
        let mut fills = match self.ibkr_client.executions(&account, date).await {
            Ok(r) => r,
            Err(e) => return map_tool_result::<(), String>(Err(e.to_string())),
        };
        if let Some(filter) = args.symbol.as_deref() {
            let needle = filter.to_uppercase();
            fills.retain(|f| f.symbol.eq_ignore_ascii_case(&needle));
        }
        let legs = match_legs(&fills);
        let totals = compute_totals(&legs);
        map_tool_result::<_, String>(Ok(json!({
            "date": date.to_string(),
            "account": account,
            "legs": legs,
            "totals": totals,
        })))
    }
}

#[cfg(test)]
mod tests {
    // imports + helpers as in executions.rs::tests
    // ... (canonical setup omitted for brevity; mirror the existing pattern)
}
```

> **Tests file:** copy the test setup pattern from `executions.rs::tests` verbatim, replacing the assertion targets. Don't rewrite `make_db` or `handler_for_mock_ibkr` — they're shared in `mcp::tools::test_support`.

- [ ] **Step 3: Wire the tool into the handler**

In `mcp/handler.rs`, add the new router to the chain:

```rust
// existing routers chained via .merge(...) or similar — add:
.merge(McpHandler::get_trade_legs_router())
```

Look at how `executions_router` is registered for the exact pattern.

- [ ] **Step 4: Run the tool test**

```bash
cd src-tauri && cargo test mcp::tools::get_trade_legs
```

Expected: PASS.

- [ ] **Step 5: Run the full suite**

```bash
cd src-tauri && cargo test
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/mcp/tools/get_trade_legs.rs src-tauri/src/mcp/tools/mod.rs src-tauri/src/mcp/handler.rs
git commit -m "feat(mcp): get_trade_legs tool — FIFO leg view with totals"
```

### Task 8: Read-only audit invariant

**Files:**
- Modify: `src-tauri/src/mcp/tools/get_trade_legs.rs::tests`

- [ ] **Step 1: Add the test mirroring `executions.rs::get_executions_does_not_write_audit`**

```rust
#[tokio::test]
async fn get_trade_legs_does_not_write_audit() {
    let (_tmp, db) = make_db();
    let mock = Arc::new(MockIbkrClient::new());
    mock.set_accounts(vec!["U1".into()]).await;
    let handler = handler_for_mock_ibkr(Arc::clone(&db), mock).await;

    let _ = handler
        .get_trade_legs(Parameters(GetTradeLegsArgs {
            account: Some("U1".into()),
            date: Some("2026-05-04".into()),
            symbol: None,
        }))
        .await
        .expect("rmcp Ok");

    let audits = crate::services::mcp_audit::list(&db, 100, 0).await.expect("list");
    assert!(audits.is_empty(), "got {:?}", audits);
}
```

- [ ] **Step 2: Run, verify it passes**

```bash
cd src-tauri && cargo test get_trade_legs_does_not_write_audit
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/mcp/tools/get_trade_legs.rs
git commit -m "test(mcp): assert get_trade_legs writes no audit rows"
```

## Exit criteria

- [ ] `match_legs` correctly handles: simple round-trip, scaled-in (multi-buy single-sell), scaled-out (single-buy multi-sell), full ladder (multi-buy multi-sell), carryover-only.
- [ ] `compute_totals` matches per-leg sum + symbol bucketing.
- [ ] `get_trade_legs` MCP tool returns a structured `{date, account, legs[], totals}` envelope.
- [ ] Tracer-bullet: from a Claude Code session, `get_trade_legs(date='YYYY-MM-DD' /* yesterday */)` returns leg-by-leg P&L matching IBKR Trade Log to ±$0.01 (verified manually against TWS).
- [ ] Read-only audit invariant: tool writes 0 `mcp_audit` rows.
- [ ] `cargo clippy --all-targets -- -D warnings` clean.
- [ ] Update master Phase 2 row + this Status header to `done (commit <sha>, YYYY-MM-DD)`.

## Gotchas

- **Floating-point hashing for `strike`.** `f64` doesn't impl `Eq` / `Hash`. The `strike_bits: Option<u64>` workaround uses bit-pattern equality, which is correct for IBKR's strike values (always a finite, exact decimal-equivalent f64). Don't use `OrderedFloat` here unless a peer service already pulls it in.
- **Multiplier parsing.** `"100".parse::<f64>()` succeeds; `None` falls back to 1.0 (correct for STK). If IBKR ever returns `"100.0"` it parses fine; `""` falls to 1.0 (a STK-shaped row).
- **Partial close carryover.** When a close consumes only part of a single open, the remainder must stay in the FIFO queue (the implementation does `push_front(front)`). A unit test for this case is recommended; not strictly required for v1 round-trips.
- **Commission proportional allocation.** When a partial open is consumed, its commission is apportioned by `(consumed_qty / original_qty)`. The leftover keeps the rest. Don't double-charge.
- **Sort key for legs.** Sort by `opened_at` for chronological narrative. The session that produced this plan implicitly assumed this order when summarising "Trade #1 was excellent, then Trade #2..."
- **Short-side support.** v1 emits one leg per fill tagged `complex_strategy` for shorts. If the user starts shorting regularly, fold a symmetric matcher in; mirror the long-side queue with sides flipped.
- **Combo orders.** The `order_ids_seen.len() < group.len()` heuristic is rough. v1 doesn't tag legs as `complex_strategy` from this; the heuristic exists only to enable a later refinement. Don't ship a half-implemented combo detector.
- **Wire DTO stability.** `TradeLeg` is the contract for `get_trade_legs` AND for `day_reviews.leg_observations` (Phase 4) AND for the future Tauri command (Phase 7). Renaming a field anywhere breaks three consumers. Pin via a serde round-trip test if you find yourself renaming.
