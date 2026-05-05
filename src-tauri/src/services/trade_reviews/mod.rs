//! Phase 4 (quant-decisions): R-edge + discipline scoring.
//!
//! Layers:
//! - [`tags`] — closed `BehavioralTag` enum the LLM picks from.
//! - [`grade`] — pure v2 scoring: `compute_score_v2`, `compute_discipline_v2`,
//!   `ConvictionCalibration`. The pre-P4 `compute_grade` (`net_pnl/100 +
//!   tag_weights`) is intentionally absent — the CI grep invariant
//!   forbids the `net_pnl/100` term in this subtree.
//! - [`equity_curve`] — pure daily equity-series reconstruction.
//! - [`risk_metrics`] — pure Sharpe / Sortino / Calmar / PF /
//!   expectancy / max-DD computation.
//! - [`attribution`] — per-strategy roll-up.
//! - [`store`] — `TradeReviewStore` (UPSERT + reads, both v1 and v2).
//!
//! The agent (`agent/eod_review.py`) gathers fills, asks the LLM for
//! tags + narrative, then hands the bundle to the `write_trade_review`
//! MCP rail. The rail joins setup-id linkage to compute `score_v2`
//! and writes both `score_v2` + `discipline_v2` separately — the
//! composite is shown but never summed for ranking.

pub mod attribution;
pub mod equity_curve;
pub mod generator;
pub mod grade;
pub mod risk_metrics;
pub mod scoring;
pub mod store;
pub mod tags;
pub mod types;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use attribution::{rollup_by_strategy, LegWithR, StrategyRollup};
#[allow(unused_imports)]
pub use equity_curve::{reconstruct_daily_equity, EquityPoint};
#[allow(unused_imports)]
pub use generator::PROMPT_VERSION_RUST;
#[allow(unused_imports)]
pub use generator::{GenerateError, TradeReviewGenerator};
#[allow(unused_imports)]
pub use grade::{
    compute_discipline_v2, compute_score_v2, ConvictionCalibration, Grade, GradeLetter, ScoreV2,
};
#[allow(unused_imports)]
pub use risk_metrics::{compute_risk_metrics, RiskMetrics, DEFAULT_RISK_FREE_RATE_ANNUAL};
#[allow(unused_imports)]
pub use scoring::{compute_v2_fields, V2ComputeInputs};
#[allow(unused_imports)]
pub use store::{TradeReviewError, TradeReviewStore, WriteOutcome};
#[allow(unused_imports)]
pub use tags::BehavioralTag;
#[allow(unused_imports)]
pub use types::{
    LegObservation, LegSummary, ReviewV2Fields, TradeReview, WriteTradeReviewRequest,
};
