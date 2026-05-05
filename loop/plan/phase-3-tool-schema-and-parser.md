# Phase 3 — Tool schema + response parser (`generator/tool.rs`)

> Part of [In-app trade-review generator](master.md). See master for invariants.

**Status:** done (commit 704f80f, 2026-05-05)

**Depends on:** Phase 2 (`generator/` submodule scaffold).

**Goal:** Produce two artifacts:

1. A `submit_trade_review` `ToolSchema` value that mirrors the Python `TRADE_REVIEW_TOOL_SCHEMA` and uses `BehavioralTag::ALL` for the closed enum.
2. A pure parser `parse_tool_response(&Value) -> Result<ParsedReview, ParseError>` that turns the tool-call input JSON into typed `(behavioral_tags, leg_observations, narrative_md)` and defensively drops unknown enum values (belt-and-braces against an LLM that ignores the schema).

## Why this matters

The orchestrator (Phase 5) builds the `LlmRequest` from this schema and parses the response with this function. Keeping it isolated from the orchestrator means we can test schema/parse logic without standing up an LLM seam.

## Files

**Create:**
- `src-tauri/src/services/trade_reviews/generator/tool.rs` — schema + parser + tests.

**Modify:**
- `src-tauri/src/services/trade_reviews/generator/mod.rs` — add `pub mod tool;`.

## Files to read before editing

- `agent/trade_review.py` lines 100–189 — `TRADE_REVIEW_TOOL_NAME`, `TRADE_REVIEW_TOOL_SCHEMA`, `parse_tool_response` reference.
- `src-tauri/src/services/llm_service/types.rs` — `ToolSchema`, `ToolCall` (the wire types).
- `src-tauri/src/services/trade_reviews/types.rs` — `LegObservation` (parser output type).
- `src-tauri/src/services/trade_reviews/tags.rs` — `BehavioralTag` (closed enum).

## Steps

- [ ] **Step 1: Add the module declaration.**

Edit `src-tauri/src/services/trade_reviews/generator/mod.rs` and add under the existing `pub mod prompt;`:

```rust
pub mod tool;
```

- [ ] **Step 2: Write the failing tests.**

Create `src-tauri/src/services/trade_reviews/generator/tool.rs`:

```rust
//! `submit_trade_review` tool schema + response parser.
//!
//! Mirrors `agent/trade_review.py::TRADE_REVIEW_TOOL_SCHEMA` /
//! `parse_tool_response`. The schema is consumed by Phase 5's
//! orchestrator when building the `LlmRequest`; the parser turns the
//! tool-call input back into typed Rust values.

use serde_json::{json, Value};
use thiserror::Error;

use crate::services::llm_service::ToolSchema;
use crate::services::trade_reviews::tags::BehavioralTag;
use crate::services::trade_reviews::types::LegObservation;

pub const TOOL_NAME: &str = "submit_trade_review";

/// Result of a successful parse. The caller composes a
/// `WriteTradeReviewRequest` from this + the pre-computed `LegSummary`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedReview {
    pub behavioral_tags: Vec<BehavioralTag>,
    pub leg_observations: Vec<LegObservation>,
    pub narrative_md: String,
}

#[derive(Error, Debug, PartialEq)]
pub enum ParseError {
    #[error("missing or empty narrative_md")]
    EmptyNarrative,
    #[error("malformed input: expected JSON object")]
    NotAnObject,
}

pub fn submit_trade_review_schema() -> ToolSchema {
    // Implemented in Step 4.
    ToolSchema {
        name: String::new(),
        description: String::new(),
        input_schema: Value::Null,
    }
}

pub fn parse_tool_response(_input: &Value) -> Result<ParsedReview, ParseError> {
    // Implemented in Step 4.
    Err(ParseError::NotAnObject)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_names() -> Vec<String> {
        BehavioralTag::ALL
            .iter()
            .map(|t| serde_json::to_string(t).unwrap().trim_matches('"').to_string())
            .collect()
    }

    #[test]
    fn schema_has_correct_name_and_closed_tag_enum() {
        let schema = submit_trade_review_schema();
        assert_eq!(schema.name, "submit_trade_review");
        assert!(!schema.description.is_empty());

        // behavioral_tags items.enum is the same set as BehavioralTag::ALL.
        let tags_enum = schema.input_schema["properties"]["behavioral_tags"]["items"]["enum"]
            .as_array()
            .expect("enum array");
        let mut got: Vec<String> = tags_enum
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        got.sort();
        let mut want = tag_names();
        want.sort();
        assert_eq!(got, want);

        // narrative_md is required; behavioral_tags is required.
        let required = schema.input_schema["required"]
            .as_array()
            .expect("required");
        let required: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required.contains(&"behavioral_tags"));
        assert!(required.contains(&"narrative_md"));
        // additionalProperties=false on the root.
        assert_eq!(schema.input_schema["additionalProperties"], json!(false));
    }

    #[test]
    fn parses_well_formed_response() {
        let input = json!({
            "behavioral_tags": ["flat_close", "discipline_on_loser"],
            "leg_observations": [
                {"leg_id": "leg_1", "observation_md": "Held into earnings.", "tag": "flat_close"},
                {"leg_id": "leg_2", "observation_md": "Cut loser fast."}
            ],
            "narrative_md": "  Solid day.  "
        });
        let parsed = parse_tool_response(&input).expect("ok");
        assert_eq!(
            parsed.behavioral_tags,
            vec![BehavioralTag::FlatClose, BehavioralTag::DisciplineOnLoser]
        );
        assert_eq!(parsed.leg_observations.len(), 2);
        assert_eq!(parsed.leg_observations[0].leg_id, "leg_1");
        assert_eq!(
            parsed.leg_observations[0].tag,
            Some(BehavioralTag::FlatClose)
        );
        assert_eq!(parsed.leg_observations[1].tag, None);
        assert_eq!(parsed.narrative_md, "Solid day.");
    }

    #[test]
    fn drops_unknown_tag_values_defensively() {
        let input = json!({
            "behavioral_tags": ["flat_close", "totally_made_up_tag", 42, null, "off_thesis_trade"],
            "narrative_md": "x"
        });
        let parsed = parse_tool_response(&input).expect("ok");
        assert_eq!(
            parsed.behavioral_tags,
            vec![BehavioralTag::FlatClose, BehavioralTag::OffThesisTrade]
        );
        assert!(parsed.leg_observations.is_empty());
    }

    #[test]
    fn drops_observations_with_missing_or_wrong_typed_fields() {
        let input = json!({
            "behavioral_tags": [],
            "leg_observations": [
                {"leg_id": "ok", "observation_md": "fine"},
                {"leg_id": "bad_no_md"},
                {"observation_md": "no leg_id"},
                {"leg_id": 7, "observation_md": "wrong type"},
                "not even an object"
            ],
            "narrative_md": "x"
        });
        let parsed = parse_tool_response(&input).expect("ok");
        assert_eq!(parsed.leg_observations.len(), 1);
        assert_eq!(parsed.leg_observations[0].leg_id, "ok");
    }

    #[test]
    fn rejects_missing_or_empty_narrative() {
        let input = json!({"behavioral_tags": []});
        assert_eq!(
            parse_tool_response(&input).unwrap_err(),
            ParseError::EmptyNarrative
        );

        let input = json!({"behavioral_tags": [], "narrative_md": "   "});
        assert_eq!(
            parse_tool_response(&input).unwrap_err(),
            ParseError::EmptyNarrative
        );
    }

    #[test]
    fn rejects_non_object_root() {
        let input = json!(["not", "an", "object"]);
        assert_eq!(
            parse_tool_response(&input).unwrap_err(),
            ParseError::NotAnObject
        );
    }
}
```

- [ ] **Step 3: Run the failing tests.**

Run: `cd src-tauri && cargo test --lib services::trade_reviews::generator::tool`
Expected: every test fails (the placeholder schema/parser return junk).

- [ ] **Step 4: Implement schema + parser.**

Replace the bodies of `submit_trade_review_schema` and `parse_tool_response`:

```rust
pub fn submit_trade_review_schema() -> ToolSchema {
    let tag_names: Vec<String> = BehavioralTag::ALL
        .iter()
        .map(|t| serde_json::to_string(t).unwrap().trim_matches('"').to_string())
        .collect();
    ToolSchema {
        name: TOOL_NAME.to_string(),
        description:
            "Pick behavioral tags from the closed enum and write a narrative scoring today's fills. \
             Do not pass a grade — the server computes it deterministically from the summary + your tags."
                .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "behavioral_tags": {
                    "type": "array",
                    "items": {"type": "string", "enum": tag_names.clone()},
                    "description": "Closed enum — pick only from the listed values. Empty list is allowed for an unremarkable day."
                },
                "leg_observations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "leg_id": {"type": "string"},
                            "observation_md": {"type": "string"},
                            "tag": {"type": "string", "enum": tag_names},
                        },
                        "required": ["leg_id", "observation_md"],
                        "additionalProperties": false,
                    },
                    "description": "1–3 most consequential legs of the day. Each observation is 1–2 sentences."
                },
                "narrative_md": {
                    "type": "string",
                    "description": "200–400 words of markdown commentary. No front-matter, no fenced wrappers, no headers above ###."
                },
            },
            "required": ["behavioral_tags", "narrative_md"],
            "additionalProperties": false,
        }),
    }
}

pub fn parse_tool_response(input: &Value) -> Result<ParsedReview, ParseError> {
    if !input.is_object() {
        return Err(ParseError::NotAnObject);
    }

    let behavioral_tags: Vec<BehavioralTag> = input
        .get("behavioral_tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let s = v.as_str()?;
                    serde_json::from_value::<BehavioralTag>(Value::String(s.to_string())).ok()
                })
                .collect()
        })
        .unwrap_or_default();

    let leg_observations: Vec<LegObservation> = input
        .get("leg_observations")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let obj = v.as_object()?;
                    let leg_id = obj.get("leg_id")?.as_str()?.to_string();
                    let observation_md = obj.get("observation_md")?.as_str()?.to_string();
                    let tag = obj
                        .get("tag")
                        .and_then(|t| t.as_str())
                        .and_then(|t| {
                            serde_json::from_value::<BehavioralTag>(Value::String(t.to_string())).ok()
                        });
                    Some(LegObservation {
                        leg_id,
                        symbol: None,
                        observation_md,
                        tag,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let narrative_md = input
        .get("narrative_md")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if narrative_md.is_empty() {
        return Err(ParseError::EmptyNarrative);
    }

    Ok(ParsedReview {
        behavioral_tags,
        leg_observations,
        narrative_md,
    })
}
```

- [ ] **Step 5: Run the tests to confirm green.**

Run: `cd src-tauri && cargo test --lib services::trade_reviews::generator::tool`
Expected: 6 tests pass.

- [ ] **Step 6: Pre-commit gates.**

Run: `cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/services/trade_reviews/generator/
git commit -m "$(cat <<'EOF'
feat(trade-reviews): submit_trade_review tool schema + parser

Rust mirror of agent/trade_review.py's TRADE_REVIEW_TOOL_SCHEMA and
parse_tool_response. The schema's behavioral_tags enum is sourced
from BehavioralTag::ALL (Rust enum is already the source of truth);
parser drops unknown values defensively against an LLM that ignores
the closed enum.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
