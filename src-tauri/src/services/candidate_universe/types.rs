//! Shared types for the candidate-universe staging layer.

#![allow(dead_code)] // consumers (promoter, MCP tools, scheduler) land in subsequent steps

use serde::{Deserialize, Serialize};

/// One row of `candidate_universe`. The watchlist only inherits a
/// candidate after [`crate::services::candidate_promoter`] (auto, on
/// score threshold) or the `promote_candidate` MCP tool (agent /
/// interactive) flips `promoted_at` from NULL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Candidate {
    pub symbol: String,
    /// Merged score across `sources`, currently `MAX(per_source.score)`.
    /// Promotion thresholds compare against this number.
    pub score: f64,
    /// One entry per scanner / sentiment-surge / future-earnings hit.
    /// Persisted as a JSON array; empty would mean the row is in an
    /// invalid state.
    pub sources: Vec<CandidateSource>,
    pub reason_md: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    /// Eviction deadline. The decay job deletes unpromoted rows where
    /// `decay_at <= now`. Promoted rows are kept for audit history.
    pub decay_at: i64,
    /// Unix-seconds timestamp set when the candidate was promoted
    /// (auto or agent). NULL while the candidate is still staging.
    pub promoted_at: Option<i64>,
}

/// One source-specific contribution to a candidate. Multiple of these
/// stack inside a single `Candidate.sources` array — same symbol hit
/// from `scanner_top_perc_gain` and `sentiment_surge` produces two
/// entries in one row, not two rows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateSource {
    /// Source identifier — e.g. `"scanner_top_perc_gain"`,
    /// `"sentiment_surge"`, `"earnings_movers"`. Free-form so new
    /// scanners can register without a schema change.
    pub source: String,
    /// Source-normalised score in `[0.0, 1.0]`. `1.0` is "as hot as
    /// this source ever sees". Merged into `Candidate.score` via MAX.
    pub score: f64,
    /// Optional source-specific rank (e.g. position in IBKR's
    /// TOP_PERC_GAIN response). `None` when the source doesn't expose
    /// one (sentiment surges).
    pub rank: Option<i64>,
    /// Source-specific metadata — arbitrary JSON. Keeps the primary
    /// row schema tight while still letting scanners stash whatever
    /// the agent might want to inspect (industry filter, mention
    /// counts, etc.).
    #[serde(default)]
    pub meta: serde_json::Value,
    /// Unix seconds the source last contributed. Lets the UI render
    /// "freshest source" in the candidate browser.
    pub last_seen: i64,
}

/// Input shape consumed by [`crate::services::candidate_universe::repo::upsert`].
/// Constructed by every scanner-like producer.
#[derive(Debug, Clone)]
pub struct NewCandidate {
    pub symbol: String,
    pub source: CandidateSource,
    pub reason_md: Option<String>,
    /// How many seconds from `now` this source's hit should keep the
    /// candidate alive. `Default 7d` for IBKR scanners; sentiment
    /// surges decay faster (≈48h) per the phase plan.
    pub ttl_seconds: i64,
}

/// Filter for [`crate::services::candidate_universe::repo::list`].
#[derive(Debug, Clone, Default)]
pub struct CandidateFilter {
    /// Substring-match against any source identifier (case-insensitive).
    /// `None` returns rows from every source.
    pub source_substring: Option<String>,
    /// Lower bound on merged `score` (inclusive). `None` => no floor.
    pub min_score: Option<f64>,
    /// Only rows with `last_seen >= since`. `None` => any age.
    pub since_last_seen: Option<i64>,
    /// `true` includes promoted rows; default excludes them so the
    /// browser only shows the "agent's inbox".
    pub include_promoted: bool,
    /// Hard cap on returned rows. Defaults to 100 in the read path.
    pub limit: Option<usize>,
}
