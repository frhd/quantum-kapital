//! Decision types for the decay-watcher.
//!
//! Pure data + simple constructors. The LLM-backed implementation lives
//! in `mod.rs`; this file is intentionally dependency-free so tests can
//! parse and construct decisions without booting an `LlmService`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecayOutcome {
    StillValid,
    Invalidated,
    TargetHit,
    ThesisChanged,
    /// Local-only marker for "we did not consult the LLM this tick"
    /// (too-fresh setup, budget exhausted, transport failure). Never
    /// emitted by the model.
    Skipped,
}

impl DecayOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            DecayOutcome::StillValid => "still_valid",
            DecayOutcome::Invalidated => "invalidated",
            DecayOutcome::TargetHit => "target_hit",
            DecayOutcome::ThesisChanged => "thesis_changed",
            DecayOutcome::Skipped => "skipped",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "still_valid" => Some(DecayOutcome::StillValid),
            "invalidated" => Some(DecayOutcome::Invalidated),
            "target_hit" => Some(DecayOutcome::TargetHit),
            "thesis_changed" => Some(DecayOutcome::ThesisChanged),
            "skipped" => Some(DecayOutcome::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecayDecision {
    pub still_valid: bool,
    pub outcome: DecayOutcome,
    pub reason: Option<String>,
    pub suggested_action: Option<String>,
}

impl DecayDecision {
    pub fn still_valid() -> Self {
        Self {
            still_valid: true,
            outcome: DecayOutcome::StillValid,
            reason: None,
            suggested_action: None,
        }
    }

    pub fn invalidate(reason: impl Into<String>) -> Self {
        Self {
            still_valid: false,
            outcome: DecayOutcome::Invalidated,
            reason: Some(reason.into()),
            suggested_action: None,
        }
    }

    pub fn target_hit(reason: impl Into<String>) -> Self {
        Self {
            still_valid: false,
            outcome: DecayOutcome::TargetHit,
            reason: Some(reason.into()),
            suggested_action: None,
        }
    }

    pub fn thesis_changed(reason: impl Into<String>) -> Self {
        Self {
            still_valid: false,
            outcome: DecayOutcome::ThesisChanged,
            reason: Some(reason.into()),
            suggested_action: None,
        }
    }

    pub fn skipped() -> Self {
        Self {
            still_valid: true,
            outcome: DecayOutcome::Skipped,
            reason: None,
            suggested_action: None,
        }
    }
}
