// Phase 06 lays the strategy framework. Several public items (trait methods,
// context fields, error variants, re-exports) are intentionally unused until
// Phase 07+ concrete detectors and Phase 13/14 schedulers consume them.
#![allow(dead_code, unused_imports)]

mod candidate;
mod config;
mod context;
mod indicators;
mod registry;
mod trait_def;

pub mod breakout;
pub mod episodic_pivot;
pub mod exits;
pub mod parabolic_short;

#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod tests;

pub use breakout::BreakoutDetector;
pub use candidate::{targets_for_risk_profile, Direction, SetupCandidate, SkipReason, TargetLevel};
pub use config::{BreakoutCfg, DetectorsConfig, EpisodicPivotCfg, ParabolicShortCfg};
pub use context::MarketContext;
pub use episodic_pivot::EpisodicPivotDetector;
pub use parabolic_short::ParabolicShortDetector;
pub use registry::{DetectorOutcome, DetectorRegistry};
pub use trait_def::{DetectorError, StrategyDetector};

/// Default registry seeded with all production detectors. Order matters for
/// deterministic test output and debugging. Equivalent to
/// `registry_from_config(&DetectorsConfig::default())`.
pub fn default_registry() -> DetectorRegistry {
    registry_from_config(&DetectorsConfig::default())
}

/// Build a registry whose detectors carry the supplied tunable thresholds.
/// This is the production constructor used at app boot from
/// `AppConfig.detectors`.
pub fn registry_from_config(cfg: &DetectorsConfig) -> DetectorRegistry {
    let mut reg = DetectorRegistry::new();
    reg.register(std::sync::Arc::new(BreakoutDetector::with_config(
        cfg.breakout.clone(),
    )));
    reg.register(std::sync::Arc::new(EpisodicPivotDetector::with_config(
        cfg.episodic_pivot.clone(),
    )));
    reg.register(std::sync::Arc::new(ParabolicShortDetector::with_config(
        cfg.parabolic_short.clone(),
    )));
    reg
}
