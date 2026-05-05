# Phase 4 — `LegSummary` builder (`generator/summary.rs`)

> Part of [In-app trade-review generator](master.md). See master for invariants.

**Status:** done (commit 79a6f2b, 2026-05-05)

**Depends on:** Phase 2 (submodule scaffold).

**Goal:** Pure function `summarize(legs: &[TradeLeg]) -> LegSummary`. Rust port of `agent/trade_review.py::leg_summary_from_legs`. Same numeric semantics: gross/net/commissions are sums; round-trip + carryover counts are filtered by tag; `win_rate` is winners/closed (None when no closed legs); `by_symbol` aggregates `net_pnl`.

## Why this matters

The orchestrator (Phase 5) needs a `LegSummary` to feed into both the prompt and `TradeReviewStore::write` (which computes the grade from it). Keeping summary computation off the orchestrator's critical path means a prompt regression never silently changes the persisted summary numbers.

## Files

**Create:**
- `src-tauri/src/services/trade_reviews/generator/summary.rs` — `summarize` + tests.

**Modify:**
- `src-tauri/src/services/trade_reviews/generator/mod.rs` — add `pub mod summary;`.

## Files to read before editing

- `agent/trade_review.py` lines 41–90 — `leg_summary_from_legs` reference.
- `src-tauri/src/services/trade_legs/types.rs` — `TradeLeg`, `LegTag`.
- `src-tauri/src/services/trade_legs/fifo.rs::compute_totals` — already computes some of these aggregates over `TradeLeg`. **Read it.** If it produces a `LegTotals` superset of what we need, prefer composing on top instead of duplicating loops.

## Steps

- [ ] **Step 1: Inspect `compute_totals`.**

```bash
grep -n "pub fn compute_totals" src-tauri/src/services/trade_legs/fifo.rs
```

Read its body. Two relevant facts:

- `LegTotals` exposes `gross_pnl`, `net_pnl`, `commissions`, `n_round_trips`, `n_carryover`, `by_symbol: BTreeMap<String, SymbolTotals>`. Almost a superset.
- It does NOT compute `win_rate`. We compute that in the new module.

The summary builder will compose `compute_totals` for the shared aggregates and add the win-rate calculation + flatten `by_symbol` to `BTreeMap<String, f64>` (the `LegSummary` shape).

- [ ] **Step 2: Add the module declaration.**

Edit `src-tauri/src/services/trade_reviews/generator/mod.rs`:

```rust
pub mod summary;
```

- [ ] **Step 3: Write the failing tests.**

Create `src-tauri/src/services/trade_reviews/generator/summary.rs`:

```rust
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
    // Implemented in Step 5.
    let _ = legs;
    LegSummary {
        gross_pnl: 0.0,
        net_pnl: 0.0,
        commissions_total: 0.0,
        n_round_trips: 0,
        n_carryover: 0,
        win_rate: None,
        by_symbol: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn leg(
        leg_id: &str,
        symbol: &str,
        gross: f64,
        comm: f64,
        net: f64,
        tag: LegTag,
    ) -> TradeLeg {
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
            sell_qty: if matches!(tag, LegTag::RoundTrip) { 100.0 } else { 0.0 },
            avg_sell_price: if matches!(tag, LegTag::RoundTrip) { 101.0 } else { 0.0 },
            gross_pnl: gross,
            commission_total: comm,
            net_pnl: net,
            hold_minutes: matches!(tag, LegTag::RoundTrip).then_some(60),
            source_exec_ids: vec![],
            tags: vec![tag],
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
```

- [ ] **Step 4: Run the failing tests.**

Run: `cd src-tauri && cargo test --lib services::trade_reviews::generator::summary`
Expected: 4 tests fail.

- [ ] **Step 5: Implement `summarize`.**

Replace the placeholder body:

```rust
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
```

- [ ] **Step 6: Run the tests to confirm green.**

Run: `cd src-tauri && cargo test --lib services::trade_reviews::generator::summary`
Expected: 4 tests pass.

- [ ] **Step 7: Pre-commit gates.**

Run: `cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit.**

```bash
git add src-tauri/src/services/trade_reviews/generator/
git commit -m "$(cat <<'EOF'
feat(trade-reviews): summarize TradeLegs into a LegSummary

Pure function over the FIFO-matched legs from
services::trade_legs::match_legs. Composes compute_totals for the
shared aggregates and adds win_rate (winners / closed; None when no
closed legs) and a flattened by_symbol map keyed by symbol → net P&L.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
