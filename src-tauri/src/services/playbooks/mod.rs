//! Phase 5 — structured pre-market playbooks.
//!
//! Two layers:
//! - [`types`] — wire types (`RankedSetup`, `SkipEntry`, `Playbook`).
//! - [`store`] — `PlaybookStore` (auto-incrementing `generation_id` + reads).
//!
//! The agent (`agent/morning_sweep.py`) runs a forced-tool LLM call to
//! produce `(ranked_setups, skip_list)`, then forwards them via the
//! `write_playbook` MCP rail. The morning_pack remains a sibling output
//! so research-notes prose and orders-shaped playbooks coexist.

pub mod store;
pub mod types;

#[cfg(test)]
mod tests;

#[allow(unused_imports)] // public API surface — used by mcp tools and (future) FE wrappers
pub use store::{PlaybookError, PlaybookStore, WriteOutcome};
#[allow(unused_imports)]
pub use types::{
    Conviction, EvidenceRef, Playbook, RankedSetup, SetupBias, SkipEntry, WritePlaybookRequest,
};
