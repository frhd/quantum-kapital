//! Phase 4 — structured trade reviews.
//!
//! Three layers:
//! - [`tags`] — closed `BehavioralTag` enum the LLM picks from.
//! - [`grade`] — pure deterministic `compute_grade(summary, tags) -> Grade`.
//! - [`store`] — `TradeReviewStore` (idempotent UPSERT + reads).
//!
//! The agent (`agent/eod_review.py`) gathers fills, asks the LLM for
//! tags + narrative through a forced-tool call, then hands the bundle
//! to the `write_trade_review` MCP rail. The rail computes the grade
//! server-side — the LLM never picks the grade.

pub mod generator;
pub mod grade;
pub mod store;
pub mod tags;
pub mod types;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use generator::PROMPT_VERSION_RUST;
#[allow(unused_imports)] // public API surface — used by mcp tools and (future) FE wrappers
pub use grade::{compute_grade, Grade, GradeLetter};
#[allow(unused_imports)]
pub use store::{TradeReviewError, TradeReviewStore, WriteOutcome};
#[allow(unused_imports)]
pub use tags::BehavioralTag;
#[allow(unused_imports)]
pub use types::{LegObservation, LegSummary, TradeReview, WriteTradeReviewRequest};
