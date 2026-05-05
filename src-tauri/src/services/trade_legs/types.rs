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
    ComplexStrategy, // multi-leg combo heuristic (same order_id across legs)
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
    /// Phase 2 linkage: detector class string carried from the
    /// underlying executions' `strategy`. `None` when no fill in the
    /// leg was attributed to a setup. A leg whose source fills span
    /// multiple strategies is rare in practice; when it happens, the
    /// FIFO matcher records the strategy of the **opening** fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// Phase 2 linkage: setup id of the opening fill. Same NULL
    /// semantics as `strategy`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_id: Option<i64>,
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
