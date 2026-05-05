//! Phase 6 — `get_trader_profile` aggregator.
//!
//! Pure SQL aggregate over `day_reviews`. No LLM, no IBKR. Composed by
//! the `get_trader_profile` MCP read tool and consumed by
//! `agent/morning_sweep.py` to condition the playbook on the trader's
//! recent behavioral history.

pub mod aggregator;
pub mod types;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use aggregator::aggregate;
#[allow(unused_imports)]
pub use types::{
    PnlByTag, RecentIncident, TagFrequency, TraderProfile, Trendline, WindowSummary,
};
