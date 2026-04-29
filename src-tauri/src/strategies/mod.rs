// Phase 06 lays the strategy framework. Several public items (trait methods,
// context fields, error variants, re-exports) are intentionally unused until
// Phase 07+ concrete detectors and Phase 13/14 schedulers consume them.
#![allow(dead_code, unused_imports)]

mod candidate;
mod context;
mod registry;
mod trait_def;

#[cfg(test)]
mod tests;

pub use candidate::{targets_for_risk_profile, Direction, SetupCandidate, TargetLevel};
pub use context::MarketContext;
pub use registry::{DetectorOutcome, DetectorRegistry};
pub use trait_def::{DetectorError, StrategyDetector};
