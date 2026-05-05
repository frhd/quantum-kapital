//! Phase 2 — `services/tca/` value types.
//!
//! Shared between `intent.rs` (DB I/O), `matcher.rs` (pure linkage),
//! `attribution.rs` (rollup queries) and the public API.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Side of an order intent. Disjoint from `ibkr::types::ExecutionSide`
/// (`bought`/`sold`) because intents are recorded *before* the fill
/// arrives and the trader's directional intent is naturally
/// expressed as buy/sell rather than past-tense bought/sold. The
/// matcher (`matcher.rs`) maps `Buy ↔ Bought`, `Sell ↔ Sold`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IntentSide {
    Buy,
    Sell,
}

impl IntentSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            IntentSide::Buy => "buy",
            IntentSide::Sell => "sell",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "buy" => Some(IntentSide::Buy),
            "sell" => Some(IntentSide::Sell),
            _ => None,
        }
    }
}

/// Where the intended price came from. Recorded alongside the intent
/// so attribution analyses can filter ("show me only setups where the
/// trigger price was the intent" — the cleanest signal for breakouts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntendedPriceSource {
    /// `SetupCandidate.trigger_price` at order-confirm time. Default
    /// when an intent is recorded with `setup_id`.
    TriggerPrice,
    /// Live mid quote at order-placement, used when the trigger is
    /// stale (>5 min old) or missing.
    LiveQuote,
    /// Limit price the user typed — last-resort source if no quote
    /// and no trigger price are available.
    LimitPrice,
    /// Out-of-band: trader recorded an intent retroactively for a
    /// fill that landed without one.
    Manual,
}

impl IntendedPriceSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            IntendedPriceSource::TriggerPrice => "trigger_price",
            IntendedPriceSource::LiveQuote => "live_quote",
            IntendedPriceSource::LimitPrice => "limit_price",
            IntendedPriceSource::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "trigger_price" => Some(IntendedPriceSource::TriggerPrice),
            "live_quote" => Some(IntendedPriceSource::LiveQuote),
            "limit_price" => Some(IntendedPriceSource::LimitPrice),
            "manual" => Some(IntendedPriceSource::Manual),
            _ => None,
        }
    }
}

/// Open / matched / expired. Drives the matcher's lookup index — only
/// `open` rows are scanned for a fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentStatus {
    Open,
    Matched,
    Expired,
}

impl IntentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            IntentStatus::Open => "open",
            IntentStatus::Matched => "matched",
            IntentStatus::Expired => "expired",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(IntentStatus::Open),
            "matched" => Some(IntentStatus::Matched),
            "expired" => Some(IntentStatus::Expired),
            _ => None,
        }
    }
}

/// One row in `order_intents`. Recorded the moment the trader
/// confirms an order in our UI; matched against fills as they arrive.
///
/// `qty` is the *intended* quantity. `matched_qty` accumulates as
/// child fills arrive — partial fills leave the intent open until
/// `matched_qty == qty`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderIntent {
    pub intent_id: String,
    /// `None` for retroactive manual intents recorded against an
    /// out-of-band TWS fill.
    pub setup_id: Option<i64>,
    pub account: String,
    pub symbol: String,
    pub side: IntentSide,
    pub qty: f64,
    pub intended_price_cents: i64,
    pub intended_price_source: IntendedPriceSource,
    pub posted_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub status: IntentStatus,
    pub matched_qty: f64,
}

impl OrderIntent {
    #[allow(dead_code)] // surfaced by future read APIs (P3, attribution UI)
    pub fn intended_price(&self) -> f64 {
        self.intended_price_cents as f64 / 100.0
    }

    pub fn remaining_qty(&self) -> f64 {
        (self.qty - self.matched_qty).max(0.0)
    }
}

/// Per-side default match window. Markets fill near-instantly, but a
/// LIMIT order may sit for the full hour before the trader cancels.
/// MARKET defaults are tighter to avoid an "old market intent
/// matching a coincidental same-shape fill an hour later" failure
/// mode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MatchWindow {
    pub limit_minutes: u32,
    pub market_minutes: u32,
}

impl Default for MatchWindow {
    fn default() -> Self {
        Self {
            limit_minutes: 60,
            market_minutes: 5,
        }
    }
}

impl MatchWindow {
    #[allow(dead_code)] // tests + future P3 bracket window
    pub fn duration_for(&self, is_market: bool) -> Duration {
        let m = if is_market {
            self.market_minutes
        } else {
            self.limit_minutes
        };
        Duration::minutes(i64::from(m))
    }
}

/// A linkage decision the matcher emits per fill. The ingestor turns
/// these into `executions` UPDATE statements + an `order_intents`
/// `matched_qty` bump. Pure data — no DB handle.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkageDecision {
    pub exec_id: String,
    pub intent_id: String,
    pub setup_id: Option<i64>,
    pub intended_price_cents: i64,
    pub intended_price_source: IntendedPriceSource,
    pub slippage_bps: i64,
    /// Signed slippage in cents-per-share. Long: positive ↔ paid more
    /// than intended (cost). Short: positive ↔ received less than
    /// intended (also a cost). Both convey "trader cost" with the
    /// same positive-bad sign convention.
    pub slippage_signed: i64,
    /// Cumulative qty (across this and prior matched fills) that
    /// will be on the intent after this update. The store uses this
    /// to flip status ↔ `matched` when `>= intent.qty`.
    pub new_matched_qty: f64,
}

/// Output of `compute_slippage` — kept separate from `LinkageDecision`
/// so the pure math is testable without constructing intent IDs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlippageRecord {
    pub bps: i64,
    pub signed_cents_per_share: i64,
}

/// One row from `tca_get_attribution`. Per detector class (or
/// "unattributed" bucket).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributionRow {
    /// Detector class, e.g. "breakout". `None` ↔ unattributed bucket
    /// (pre-P2 fills, out-of-band TWS fills with no manual intent).
    pub strategy: Option<String>,
    pub n_trades: i64,
    pub gross_pnl_cents: i64,
    pub net_pnl_cents: i64,
    pub avg_slippage_bps: f64,
    pub n_with_slippage: i64,
    /// Sum of realized PnL on closing fills only — NULL realized_pnl
    /// rows (opening legs) are excluded.
    pub realized_pnl_cents: i64,
}

/// One bucket of the slippage histogram (`tca_get_slippage_distribution`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlippageBucket {
    /// Inclusive lower bound in bps (always non-negative — distribution
    /// is over absolute slippage).
    pub lower_bps: i64,
    /// Exclusive upper bound. Top bucket uses `i64::MAX`.
    pub upper_bps: i64,
    pub n: i64,
}

/// Full distribution row keyed by strategy + symbol-liquidity bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlippageDistributionRow {
    pub strategy: Option<String>,
    /// Symbol-liquidity bucket label. v1 uses a single "all" bucket so
    /// the panel renders without a liquidity-data dependency; the
    /// query is shaped to support per-bucket histograms once a
    /// liquidity classifier lands.
    pub liquidity_bucket: String,
    pub buckets: Vec<SlippageBucket>,
}

/// Default histogram buckets (in bps): 0, 1-5, 5-10, 10-25, 25-50,
/// 50-100, 100+. Tuned to retail equity slippage — most fills land
/// inside 10 bps; the 50+ tail flags structural problems.
pub fn default_histogram_edges() -> Vec<(i64, i64)> {
    vec![
        (0, 1),
        (1, 5),
        (5, 10),
        (10, 25),
        (25, 50),
        (50, 100),
        (100, i64::MAX),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_side_round_trips() {
        for s in [IntentSide::Buy, IntentSide::Sell] {
            assert_eq!(IntentSide::parse(s.as_str()), Some(s));
        }
        assert_eq!(IntentSide::parse("bought"), None);
    }

    #[test]
    fn intended_price_source_round_trips() {
        for s in [
            IntendedPriceSource::TriggerPrice,
            IntendedPriceSource::LiveQuote,
            IntendedPriceSource::LimitPrice,
            IntendedPriceSource::Manual,
        ] {
            assert_eq!(IntendedPriceSource::parse(s.as_str()), Some(s));
        }
        assert_eq!(IntendedPriceSource::parse("nope"), None);
    }

    #[test]
    fn match_window_market_is_tighter_than_limit() {
        let w = MatchWindow::default();
        assert!(w.duration_for(true) < w.duration_for(false));
    }

    #[test]
    fn intent_remaining_qty_clamps_at_zero() {
        let intent = OrderIntent {
            intent_id: "x".to_string(),
            setup_id: None,
            account: "DU1".to_string(),
            symbol: "AAPL".to_string(),
            side: IntentSide::Buy,
            qty: 100.0,
            intended_price_cents: 10_000,
            intended_price_source: IntendedPriceSource::Manual,
            posted_at: Utc::now(),
            expires_at: Utc::now(),
            status: IntentStatus::Open,
            matched_qty: 150.0,
        };
        assert_eq!(intent.remaining_qty(), 0.0);
    }
}
