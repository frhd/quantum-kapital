//! In-app trade-review generator.
//!
//! Sub-modules: [`prompt`] (pure formatter), `tool` (Phase 3),
//! `summary` (Phase 4), and an orchestrator that wires them to
//! `LlmService` + `TradeReviewStore` (Phase 5).

#![allow(dead_code)] // Phase 2: only `prompt` is wired so far.

pub mod prompt;
pub mod summary;
pub mod tool;

/// Prompt-version sentinel for the Rust generator. Bump when:
/// - the rubric weights in `tags::BehavioralTag::weight()` change,
/// - the tag enum gains/loses a value,
/// - or the prompt body in [`prompt`] changes materially.
///
/// Distinct from any future Python bump — the two paths version
/// independently and persist as separate rows in `day_reviews`.
pub const PROMPT_VERSION_RUST: i32 = 1;
