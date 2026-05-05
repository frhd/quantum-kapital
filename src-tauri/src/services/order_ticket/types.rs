//! Phase 3 — `services/order_ticket/` value types.
//!
//! `OrderTicket::with_brackets` produces a [`TicketReceipt`] on success
//! and persists a [`BracketGroupRecord`] to the `bracket_groups` table.
//! Both use the same `TargetSpec` ladder so the modal, the bracket
//! placer, and the audit row see byte-identical rungs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One rung of the take-profit ladder. `qty` is whole-share; the
/// command computes it from `qty_pct` against the parent `qty` once
/// at intent time and writes the absolute number here so re-reads
/// from `bracket_groups` reproduce the modal exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetSpec {
    /// Display label (e.g. "1R", "2R", "runner"). Mirrors
    /// `TargetLevel.label` from the detector pipeline.
    pub label: String,
    pub price: f64,
    /// Whole-share qty for this rung. Sum across rungs equals the
    /// parent qty (last rung absorbs the rounding remainder).
    pub qty: u32,
    /// Percentage of parent qty assigned to this rung. Persisted so
    /// post-hoc audits can tell whether 50/30/20 was the static
    /// default or a future config-driven shape.
    pub qty_pct: u8,
}

/// What `OrderTicket::with_brackets` returns to the Tauri command on
/// success. Carries the parent + child IBKR order ids so the UI can
/// render "your bracket is live" and link out to TWS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TicketReceipt {
    pub parent_order_id: i32,
    pub stop_order_id: i32,
    pub target_order_ids: Vec<i32>,
    pub intent_id: String,
    pub setup_id: i64,
    pub placed_at: DateTime<Utc>,
}

/// Lifecycle of a placed bracket. Updated by the post-fill reconciler
/// (P3 only writes `Open`; partial / stopped / canceled / filled flips
/// happen via `order_ticket_cancel_bracket` and the future fill-status
/// stream). Stored as `as_str()` in `bracket_groups.last_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BracketStatus {
    Open,
    Partial,
    Filled,
    Stopped,
    Canceled,
}

impl BracketStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BracketStatus::Open => "open",
            BracketStatus::Partial => "partial",
            BracketStatus::Filled => "filled",
            BracketStatus::Stopped => "stopped",
            BracketStatus::Canceled => "canceled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(BracketStatus::Open),
            "partial" => Some(BracketStatus::Partial),
            "filled" => Some(BracketStatus::Filled),
            "stopped" => Some(BracketStatus::Stopped),
            "canceled" => Some(BracketStatus::Canceled),
            _ => None,
        }
    }
}

/// One row in `bracket_groups`. Read by `order_ticket_status` and
/// the trader-profile rollup. Money fields stay in integer cents to
/// dodge SQLite REAL drift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BracketGroupRecord {
    pub parent_order_id: i32,
    pub setup_id: i64,
    pub intent_id: String,
    pub account: String,
    pub symbol: String,
    /// "long" | "short". Mirrors `setups.direction`.
    pub direction: String,
    pub parent_qty: u32,
    pub system_qty: u32,
    pub qty_override_reason: Option<String>,
    pub entry_limit_cents: i64,
    pub stop_order_id: i32,
    pub stop_price_cents: i64,
    pub target_order_ids: Vec<i32>,
    pub targets: Vec<TargetSpec>,
    pub placed_at: DateTime<Utc>,
    pub last_status: BracketStatus,
    pub last_status_at: DateTime<Utc>,
}

/// Static 50 / 30 / 20 ladder committed by master. Exposed as a const
/// so the modal and the service share a single source of truth — the
/// ladder is *not* configurable in P3. P7 replaces this with an
/// ATR-scaled ladder computed per-detector.
pub const STATIC_TARGET_LADDER_PCT: [u8; 3] = [50, 30, 20];

/// R-multiples each ladder rung pins to until P7 vol-adjusted exits
/// land. Master decision: "ship with 50/30/20 fixed; runner gets a
/// hard 3R limit until P7 trailing lands."
pub const STATIC_TARGET_R_MULTIPLES: [f64; 3] = [1.0, 2.0, 3.0];

/// Maximum age of `equity_at_decision` snapshot allowed at modal-send
/// time. Master decision: "hard block — modal Send disabled until
/// snapshot < 24h." 24h chosen so a Monday-morning setup against a
/// Friday-close NLV passes; a Tuesday-morning setup against the same
/// snapshot does not.
pub const MAX_EQUITY_STALENESS_HOURS: i64 = 24;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_ladder_sums_to_one_hundred() {
        let total: u8 = STATIC_TARGET_LADDER_PCT.iter().sum();
        assert_eq!(total, 100);
        assert_eq!(
            STATIC_TARGET_R_MULTIPLES.len(),
            STATIC_TARGET_LADDER_PCT.len()
        );
    }

    #[test]
    fn bracket_status_round_trips() {
        for s in [
            BracketStatus::Open,
            BracketStatus::Partial,
            BracketStatus::Filled,
            BracketStatus::Stopped,
            BracketStatus::Canceled,
        ] {
            assert_eq!(BracketStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(BracketStatus::parse("nope"), None);
    }
}
