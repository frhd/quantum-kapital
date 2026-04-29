// Phase 06 lays the strategy framework. Several public items (trait methods,
// context fields, error variants, re-exports) are intentionally unused until
// Phase 07+ concrete detectors and Phase 13/14 schedulers consume them.
#![allow(dead_code, unused_imports)]

mod candidate;
mod context;
mod indicators;
mod registry;
mod trait_def;

pub mod breakout;
pub mod episodic_pivot;
pub mod parabolic_short;

#[cfg(test)]
mod tests;

pub use breakout::BreakoutDetector;
pub use candidate::{targets_for_risk_profile, Direction, SetupCandidate, TargetLevel};
pub use context::MarketContext;
pub use episodic_pivot::EpisodicPivotDetector;
pub use parabolic_short::ParabolicShortDetector;
pub use registry::{DetectorOutcome, DetectorRegistry};
pub use trait_def::{DetectorError, StrategyDetector};

/// Default registry seeded with all production detectors. Order matters for
/// deterministic test output and debugging.
pub fn default_registry() -> DetectorRegistry {
    let mut reg = DetectorRegistry::new();
    reg.register(std::sync::Arc::new(BreakoutDetector));
    reg.register(std::sync::Arc::new(EpisodicPivotDetector));
    reg.register(std::sync::Arc::new(ParabolicShortDetector));
    reg
}
