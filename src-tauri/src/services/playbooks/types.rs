//! Wire types for the playbook subsystem.
//!
//! `Playbook` is the persisted artifact returned by `get_today_playbook`;
//! `WritePlaybookRequest` is what the MCP write rail accepts. The
//! `generation_id` is server-assigned (next-after-MAX per `(date, account)`),
//! so callers never pass it on writes.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SetupBias {
    Long,
    Short,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Conviction {
    A,
    B,
    C,
}

/// Pointer back into the briefing data the LLM used to justify a setup.
/// `source` is freeform v1 (`"news" | "bars" | "setup" | "sentiment" |
/// "fundamentals"` by convention); `note` is a short human-readable hint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceRef {
    pub source: String,
    pub note: String,
}

/// One actionable setup with concrete trigger / entry / invalidation /
/// target levels. `target_2` is optional (extension target).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RankedSetup {
    pub symbol: String,
    pub bias: SetupBias,
    pub trigger: String,
    pub entry: String,
    pub invalidation: String,
    pub target_1: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_2: Option<String>,
    pub conviction: Conviction,
    pub rationale_md: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<EvidenceRef>,
}

/// Symbol explicitly excluded from today's playbook with a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkipEntry {
    pub symbol: String,
    pub reason: String,
}

/// Persisted form of a playbook. Returned by `get_today_playbook`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playbook {
    pub date: NaiveDate,
    pub account: String,
    pub generation_id: i32,
    pub generated_at: DateTime<Utc>,
    pub ranked_setups: Vec<RankedSetup>,
    pub skip_list: Vec<SkipEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_call_id: Option<String>,
}

/// Inputs accepted by the MCP write rail. Note the absence of
/// `generation_id` — the store assigns it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct WritePlaybookRequest {
    pub date: NaiveDate,
    pub account: String,
    #[serde(default)]
    pub ranked_setups: Vec<RankedSetup>,
    #[serde(default)]
    pub skip_list: Vec<SkipEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_call_id: Option<String>,
}
