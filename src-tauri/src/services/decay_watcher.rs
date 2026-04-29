//! Phase 14 — decay-watcher stub.
//!
//! The intraday scheduler asks the decay-watcher whether each active
//! setup is still valid on every tick. Phase 14 ships a no-op stub so
//! the scheduler can be tested end-to-end against the real persistence
//! layer; Phase 18 replaces [`DecayWatcherStub`] with a real Anthropic-
//! backed implementation that reads the latest bars/news for the
//! ticker and decides whether the thesis still holds.
//!
//! The trait surface is intentionally narrow — `check(&Setup)` returns
//! a [`DecayDecision`] — so swapping in the real implementation is a
//! straight drop-in.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ibkr::types::tracker::Setup;

/// Decision returned by the decay-watcher for a single setup.
///
/// `still_valid = true` means "leave the setup alone"; `false` means
/// the scheduler should call `state_machine.mark_invalidated(id, reason)`
/// with the supplied `reason`. `suggested_action` is reserved for
/// Phase 18 (e.g. "tighten stop", "trail target") — the Phase 14
/// scheduler ignores it but it round-trips through serde for logging.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecayDecision {
    pub still_valid: bool,
    pub reason: Option<String>,
    pub suggested_action: Option<String>,
}

impl DecayDecision {
    pub fn still_valid() -> Self {
        Self {
            still_valid: true,
            reason: None,
            suggested_action: None,
        }
    }

    #[allow(dead_code)]
    pub fn invalidate(reason: impl Into<String>) -> Self {
        Self {
            still_valid: false,
            reason: Some(reason.into()),
            suggested_action: None,
        }
    }
}

/// Decay-watcher seam. The intraday scheduler holds an `Arc<dyn DecayWatcher>`
/// so production wiring (Phase 18) can swap in the real Anthropic-backed
/// impl without touching the scheduler.
#[async_trait]
pub trait DecayWatcher: Send + Sync {
    async fn check(&self, setup: &Setup) -> DecayDecision;
}

/// No-op stub used until Phase 18 lands. Always returns `still_valid = true`.
#[derive(Debug, Default, Clone)]
pub struct DecayWatcherStub;

#[async_trait]
impl DecayWatcher for DecayWatcherStub {
    async fn check(&self, _setup: &Setup) -> DecayDecision {
        DecayDecision::still_valid()
    }
}
