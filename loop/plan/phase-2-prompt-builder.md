# Phase 2 — Pure prompt builder (`generator/prompt.rs`)

> Part of [In-app trade-review generator](master.md). See master for invariants.

**Status:** done (commit 85b5487, 2026-05-05)

**Depends on:** —

**Goal:** Port `agent/trade_review.py::format_trade_review_prompt` to a pure Rust function that takes `(date, &[TradeLeg], &LegSummary)` and returns the user-message string the LLM sees. No I/O. No DB. No network. No async.

## Why this matters

The prompt is the single biggest source of behavioural drift between the Python and Rust paths. By porting it 1:1 (preserving section order, label casing, number formatting), we make it possible to compare outputs run-for-run during dogfooding. The pack-ideas section is omitted in v1 (master's `Defaults committed`).

## Files

**Create:**
- `src-tauri/src/services/trade_reviews/generator/mod.rs` — submodule root (mostly `pub mod` declarations + a `PROMPT_VERSION_RUST` constant).
- `src-tauri/src/services/trade_reviews/generator/prompt.rs` — the pure formatter + tests.

**Modify:**
- `src-tauri/src/services/trade_reviews/mod.rs` — add `pub mod generator;` and a `#[allow(unused_imports)] pub use generator::PROMPT_VERSION_RUST;` re-export.

## Files to read before editing

- `agent/trade_review.py` lines 195–281 — the Python `format_trade_review_prompt` reference.
- `src-tauri/src/services/trade_legs/types.rs` — `TradeLeg`, `LegTag` (the input type you'll iterate).
- `src-tauri/src/services/trade_reviews/types.rs` — `LegSummary` (the input summary type).
- `src-tauri/src/services/trade_reviews/tags.rs` — `BehavioralTag::ALL` (the tag menu source).

## Steps

- [ ] **Step 1: Create the submodule scaffold.**

Create `src-tauri/src/services/trade_reviews/generator/mod.rs`:

```rust
//! In-app trade-review generator.
//!
//! Sub-modules: [`prompt`] (pure formatter), `tool` (Phase 3),
//! `summary` (Phase 4), and an orchestrator that wires them to
//! `LlmService` + `TradeReviewStore` (Phase 5).

#![allow(dead_code)] // Phase 2: only `prompt` is wired so far.

pub mod prompt;

/// Prompt-version sentinel for the Rust generator. Bump when:
/// - the rubric weights in `tags::BehavioralTag::weight()` change,
/// - the tag enum gains/loses a value,
/// - or the prompt body in [`prompt`] changes materially.
///
/// Distinct from any future Python bump — the two paths version
/// independently and persist as separate rows in `day_reviews`.
pub const PROMPT_VERSION_RUST: i32 = 1;
```

- [ ] **Step 2: Wire the submodule into `services/trade_reviews/mod.rs`.**

Edit `src-tauri/src/services/trade_reviews/mod.rs`. After `pub mod types;` add:

```rust
pub mod generator;
```

And after the existing re-exports (`pub use grade::...` etc.), add:

```rust
#[allow(unused_imports)]
pub use generator::PROMPT_VERSION_RUST;
```

- [ ] **Step 3: Write the failing test.**

Create `src-tauri/src/services/trade_reviews/generator/prompt.rs`:

```rust
//! Pure user-message formatter for the trade-review LLM call.
//!
//! Mirrors `agent/trade_review.py::format_trade_review_prompt`. Pure
//! function over a `LegSummary`, the day's `TradeLeg`s, and the date.

use chrono::NaiveDate;
use std::fmt::Write;

use crate::services::trade_legs::{LegTag, TradeLeg};
use crate::services::trade_reviews::tags::BehavioralTag;
use crate::services::trade_reviews::types::LegSummary;

/// Build the user-message body for the trade-review LLM call.
pub fn format_prompt(date: NaiveDate, legs: &[TradeLeg], summary: &LegSummary) -> String {
    // Implemented in Step 5.
    let _ = (date, legs, summary);
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn empty_summary() -> LegSummary {
        LegSummary {
            gross_pnl: 0.0,
            net_pnl: 0.0,
            commissions_total: 0.0,
            n_round_trips: 0,
            n_carryover: 0,
            win_rate: None,
            by_symbol: Default::default(),
        }
    }

    #[test]
    fn empty_day_renders_only_summary_legs_section_header_and_tag_menu() {
        let body = format_prompt(d(2026, 5, 4), &[], &empty_summary());
        assert!(body.contains("PACK DATE: 2026-05-04"));
        assert!(body.contains("DAY SUMMARY"));
        assert!(body.contains("net_pnl:"));
        assert!(body.contains("LEGS"));
        assert!(body.contains("BEHAVIORAL TAG MENU"));
        // Tag menu must list every variant by serde name.
        for tag in BehavioralTag::ALL {
            let name = serde_json::to_string(&tag).unwrap();
            let bare = name.trim_matches('"');
            assert!(
                body.contains(bare),
                "missing tag {bare} from menu: {body}"
            );
        }
        // No win_rate line when None.
        assert!(!body.contains("win_rate:"));
        // No by_symbol section when empty.
        assert!(!body.contains("by_symbol:"));
        // No `pack_ideas` section ever (v1 omits it).
        assert!(!body.to_lowercase().contains("morning playbook"));
    }

    #[test]
    fn populated_day_renders_legs_and_per_symbol_rows() {
        let mut summary = empty_summary();
        summary.gross_pnl = 401.10;
        summary.net_pnl = 380.0;
        summary.commissions_total = 21.10;
        summary.n_round_trips = 3;
        summary.n_carryover = 1;
        summary.win_rate = Some(2.0 / 3.0);
        summary
            .by_symbol
            .insert("AAPL".to_string(), 250.0);
        summary
            .by_symbol
            .insert("TSLA".to_string(), -75.0);

        let leg = TradeLeg {
            leg_id: "leg_aapl_1".into(),
            account: "U1".into(),
            symbol: "AAPL".into(),
            contract_type: "STK".into(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            opened_at: chrono::Utc.with_ymd_and_hms(2026, 5, 4, 14, 32, 0).unwrap(),
            closed_at: Some(chrono::Utc.with_ymd_and_hms(2026, 5, 4, 15, 1, 0).unwrap()),
            buy_qty: 100.0,
            avg_buy_price: 200.0,
            sell_qty: 100.0,
            avg_sell_price: 202.50,
            gross_pnl: 250.0,
            commission_total: 2.10,
            net_pnl: 247.90,
            hold_minutes: Some(29),
            source_exec_ids: vec!["e1".into(), "e2".into()],
            tags: vec![LegTag::RoundTrip],
        };
        let body = format_prompt(d(2026, 5, 4), &[leg], &summary);

        assert!(body.contains("net_pnl:       $380.00"));
        assert!(body.contains("commissions:   $21.10"));
        assert!(body.contains("round_trips:   3"));
        assert!(body.contains("carryover:     1"));
        assert!(body.contains("win_rate:      66.7%"));
        assert!(body.contains("by_symbol:"));
        assert!(body.contains("AAPL: $+250.00"));
        assert!(body.contains("TSLA: $-75.00"));
        // Leg row.
        assert!(body.contains("leg_aapl_1 AAPL (round-trip) net=$+247.90"));
        assert!(body.contains("opened: 2026-05-04T14:32:00"));
        assert!(body.contains("closed: 2026-05-04T15:01:00"));
        assert!(body.contains("held:   29m"));
        assert!(body.contains("tags:   round_trip"));
    }

    #[test]
    fn carryover_leg_has_no_closed_line_or_pnl_line_omits_close_field() {
        let mut summary = empty_summary();
        summary.n_carryover = 1;
        let leg = TradeLeg {
            leg_id: "leg_carry".into(),
            account: "U1".into(),
            symbol: "NVDA".into(),
            contract_type: "STK".into(),
            expiry: None,
            strike: None,
            right: None,
            multiplier: None,
            opened_at: chrono::Utc.with_ymd_and_hms(2026, 5, 4, 18, 0, 0).unwrap(),
            closed_at: None,
            buy_qty: 50.0,
            avg_buy_price: 900.0,
            sell_qty: 0.0,
            avg_sell_price: 0.0,
            gross_pnl: 0.0,
            commission_total: 0.0,
            net_pnl: 0.0,
            hold_minutes: None,
            source_exec_ids: vec!["e1".into()],
            tags: vec![LegTag::Carryover],
        };
        let body = format_prompt(d(2026, 5, 4), &[leg], &summary);
        assert!(body.contains("leg_carry NVDA (carryover) net=$+0.00"));
        assert!(body.contains("opened: 2026-05-04T18:00:00"));
        assert!(!body.contains("closed:"));
        assert!(!body.contains("held:"));
    }
}
```

- [ ] **Step 4: Run the failing test.**

Run: `cd src-tauri && cargo test --lib services::trade_reviews::generator::prompt`
Expected: 3 tests fail with assertions complaining the body is empty.

- [ ] **Step 5: Implement `format_prompt`.**

Replace the `format_prompt` body in `prompt.rs`:

```rust
pub fn format_prompt(date: NaiveDate, legs: &[TradeLeg], summary: &LegSummary) -> String {
    let mut buf = String::with_capacity(1024);
    let _ = writeln!(buf, "PACK DATE: {date}");
    buf.push('\n');

    // ----- DAY SUMMARY ---------------------------------------------------
    buf.push_str("DAY SUMMARY\n");
    buf.push_str("-----------\n");
    let _ = writeln!(buf, "  net_pnl:       ${:.2}", summary.net_pnl);
    let _ = writeln!(buf, "  gross_pnl:     ${:.2}", summary.gross_pnl);
    let _ = writeln!(buf, "  commissions:   ${:.2}", summary.commissions_total);
    let _ = writeln!(buf, "  round_trips:   {}", summary.n_round_trips);
    let _ = writeln!(buf, "  carryover:     {}", summary.n_carryover);
    if let Some(wr) = summary.win_rate {
        let _ = writeln!(buf, "  win_rate:      {:.1}%", wr * 100.0);
    }
    if !summary.by_symbol.is_empty() {
        buf.push_str("  by_symbol:\n");
        for (sym, pnl) in summary.by_symbol.iter() {
            let _ = writeln!(buf, "    {sym}: ${:+.2}", pnl);
        }
    }
    buf.push('\n');

    // ----- LEGS ----------------------------------------------------------
    buf.push_str("LEGS\n");
    buf.push_str("----\n");
    for (i, leg) in legs.iter().enumerate() {
        let i = i + 1;
        let kind = if leg.tags.contains(&LegTag::RoundTrip) {
            "round-trip"
        } else if leg.tags.contains(&LegTag::Carryover) {
            "carryover"
        } else {
            "open"
        };
        let _ = writeln!(
            buf,
            "  {i}. {leg_id} {sym} ({kind}) net=${net:+.2}",
            leg_id = leg.leg_id,
            sym = leg.symbol.to_uppercase(),
            kind = kind,
            net = leg.net_pnl,
        );
        let _ = writeln!(
            buf,
            "     opened: {}",
            leg.opened_at.format("%Y-%m-%dT%H:%M:%S")
        );
        if let Some(closed) = leg.closed_at {
            let _ = writeln!(buf, "     closed: {}", closed.format("%Y-%m-%dT%H:%M:%S"));
        }
        if let Some(h) = leg.hold_minutes {
            let _ = writeln!(buf, "     held:   {h}m");
        }
        if !leg.tags.is_empty() {
            let names: Vec<String> = leg
                .tags
                .iter()
                .map(|t| {
                    serde_json::to_string(t)
                        .unwrap_or_else(|_| String::from("?"))
                        .trim_matches('"')
                        .to_string()
                })
                .collect();
            let _ = writeln!(buf, "     tags:   {}", names.join(","));
        }
    }
    buf.push('\n');

    // ----- BEHAVIORAL TAG MENU ------------------------------------------
    buf.push_str("BEHAVIORAL TAG MENU (pick zero or more from this closed enum):\n");
    for tag in BehavioralTag::ALL {
        let name = serde_json::to_string(&tag)
            .unwrap_or_else(|_| String::from("?"));
        let bare = name.trim_matches('"');
        let _ = writeln!(buf, "  - {bare}");
    }
    buf.push('\n');
    buf.push_str(
        "Call `submit_trade_review` with `behavioral_tags`, `leg_observations` (1–3 most consequential legs), and `narrative_md` (200–400 words). DO NOT pick a grade — the server computes it from the summary + your tags.\n",
    );

    buf
}
```

- [ ] **Step 6: Run the tests to confirm green.**

Run: `cd src-tauri && cargo test --lib services::trade_reviews::generator::prompt`
Expected: 3 tests pass.

- [ ] **Step 7: Pre-commit gates.**

Run: `cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit.**

```bash
git add src-tauri/src/services/trade_reviews/mod.rs src-tauri/src/services/trade_reviews/generator/
git commit -m "$(cat <<'EOF'
feat(trade-reviews): pure prompt builder for in-app generator

Rust port of agent/trade_review.py::format_trade_review_prompt. No I/O.
Adds PROMPT_VERSION_RUST = 1 sentinel so reviews authored by the new
in-app pipeline persist as a distinct prompt_version from any future
Python rubric bump.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
