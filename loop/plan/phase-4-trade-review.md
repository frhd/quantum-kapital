# Phase 4 — `day_reviews` schema + extended `eod_review.py` + `get_trade_review` / `write_trade_review` MCP tools

> Part of [Behavioral assessment via MCP](master.md). See master for invariants.

**Status:** todo

**Depends on:** 2 (`get_trade_legs` is the input)

**Goal:** Persist a structured **trade review** every trading day. The existing `agent/eod_review.py` already runs at 17:00 ET and writes a journal markdown via `append_journal_entry`; this phase extends it to ALSO score actual fills (via `get_trade_legs`), compute a deterministic grade, ask the LLM to pick behavioral tags from a closed enum and write a narrative, and persist the structured row via a new `write_trade_review` MCP rail. The new `get_trade_review(date)` read tool serves the cached row.

**Why this matters:** the grade + behavioral tags are the building blocks the trader-profile aggregator (Phase 6) uses to condition tomorrow's playbook. Without structured rows, every "rate yesterday" question forces re-derivation; with them, one MCP call returns a deterministic, cached, queryable answer.

## End-state for this phase

- `day_reviews` SQLite table exists, keyed `(date, prompt_version)`.
- `services/trade_reviews/` module exists with:
  - `tags.rs` — closed enum `BehavioralTag` (12 v1 values; weights inline).
  - `grade.rs` — pure `compute_grade(leg_summary, tags) -> Grade` returning A/B/C/D/F deterministically.
  - `store.rs` — `TradeReviewStore::write(...)` (idempotent UPSERT) and `read(date)`.
- `mcp/tools/get_trade_review.rs` registers a new read tool.
- `mcp/tools/write_trade_review.rs` registers a new agent-write rail (audited like `write_morning_pack`).
- `agent/eod_review.py` extended:
  - Fetches `get_trade_legs(yesterday)` after the existing predictions/outcomes flow.
  - Computes a leg summary client-side (or trusts a server-computed one — see Open question).
  - Asks the LLM to pick behavioral_tags (forced tool with the closed enum) AND write a narrative_md.
  - Calls `write_trade_review(date, prompt_version, tags, narrative_md, summary)` — server computes `grade` deterministically from `(summary, tags)`.
  - Continues to write the existing journal entry as a sibling output (no removal, no behaviour change to the journal).
- A Python mirror-test asserts the `BEHAVIORAL_TAGS` Python list matches the Rust `BehavioralTag` enum name-for-name.

## Files

**Create:**
- `src-tauri/src/storage/migrations/V14__day_reviews.sql` — schema below.
- `src-tauri/src/services/trade_reviews/mod.rs` — module root.
- `src-tauri/src/services/trade_reviews/tags.rs` — `BehavioralTag` enum + weights.
- `src-tauri/src/services/trade_reviews/grade.rs` — pure grade fn.
- `src-tauri/src/services/trade_reviews/store.rs` — read/write.
- `src-tauri/src/services/trade_reviews/types.rs` — `LegSummary`, `Grade`, `TradeReview`, `WriteTradeReviewRequest`.
- `src-tauri/src/services/trade_reviews/tests.rs` — unit tests (grade determinism, idempotent UPSERT, tag-weight math).
- `src-tauri/src/mcp/tools/get_trade_review.rs` — read tool.
- `src-tauri/src/mcp/tools/write_trade_review.rs` — write rail (audited).
- `agent/trade_review.py` — module: behavioral_tag enum (mirrored from Rust), prompt scaffolding, helpers for leg-summary computation. Imported by extended `eod_review.py`.
- `agent/prompts/trade_review.md` — system prompt fragment for the trade-review LLM call.
- `agent/tests/test_trade_review.py` — unit tests for the Python module.
- `agent/tests/test_tag_mirror.py` — mirror test parsing Rust enum source and asserting Python list matches.

**Modify:**
- `src-tauri/src/services/mod.rs` — add `pub mod trade_reviews;`.
- `src-tauri/src/mcp/tools/mod.rs`, `src-tauri/src/mcp/handler.rs` — register the two new tool routers.
- `agent/eod_review.py` — extend the `run_eod_review` orchestration to also produce the trade review.
- `agent/mcp_client.py` — add wrappers for the two new MCP tools (`get_trade_legs`, `get_trade_review`, `write_trade_review`).
- `agent/eod_review.py`'s `EodReviewMcp` protocol — add the new methods.
- Tests for the existing eod_review continue to pass; add new tests for the trade-review extension.

## Schema (V14__day_reviews.sql)

```sql
-- V14__day_reviews.sql
-- One structured trade review per (date, prompt_version, account).
-- prompt_version bumps when the rubric weights, tag enum, or system prompt
-- change materially; old reviews stay queryable but new versions UPSERT
-- as separate rows (so the trader-profile aggregator can group by version).

CREATE TABLE day_reviews (
    date              TEXT    NOT NULL,            -- "YYYY-MM-DD" (ET)
    account           TEXT    NOT NULL,
    prompt_version    INTEGER NOT NULL,
    generated_at      TEXT    NOT NULL,            -- ISO 8601 UTC
    grade             TEXT    NOT NULL,            -- "A"|"B"|"C"|"D"|"F"
    grade_score       REAL    NOT NULL,
    gross_pnl         REAL    NOT NULL,
    net_pnl           REAL    NOT NULL,
    commissions_total REAL    NOT NULL,
    n_round_trips     INTEGER NOT NULL,
    n_carryover       INTEGER NOT NULL,
    win_rate          REAL,                        -- nullable (no closed legs ⇒ NULL)
    behavioral_tags   TEXT    NOT NULL,            -- JSON array of enum names
    leg_observations  TEXT    NOT NULL,            -- JSON array of {leg_id, observation_md, tag?}
    narrative_md      TEXT    NOT NULL,
    llm_call_id       TEXT,                        -- foreign-key-ish to llm_calls.id
    PRIMARY KEY (date, account, prompt_version)
);

CREATE INDEX idx_day_reviews_date ON day_reviews(date);
CREATE INDEX idx_day_reviews_account_date ON day_reviews(account, date DESC);
```

## End-state types (Rust)

```rust
// services/trade_reviews/types.rs
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::tags::BehavioralTag;
use super::grade::Grade;

/// Per-leg observation surfaced into the review's `leg_observations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegObservation {
    pub leg_id: String,
    pub observation_md: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<BehavioralTag>,
}

/// Pre-computed numerical summary of a day's legs. Input to `compute_grade`
/// and to the agent's prompt. The agent does NOT recompute these — they
/// are the trusted server-side numbers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegSummary {
    pub gross_pnl: f64,
    pub net_pnl: f64,
    pub commissions_total: f64,
    pub n_round_trips: usize,
    pub n_carryover: usize,
    pub win_rate: Option<f64>,
    pub by_symbol: std::collections::BTreeMap<String, f64>, // symbol → net_pnl
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeReview {
    pub date: NaiveDate,
    pub account: String,
    pub prompt_version: i32,
    pub generated_at: DateTime<Utc>,
    pub grade: Grade,
    pub grade_score: f64,
    pub summary: LegSummary,
    pub behavioral_tags: Vec<BehavioralTag>,
    pub leg_observations: Vec<LegObservation>,
    pub narrative_md: String,
    pub llm_call_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WriteTradeReviewRequest {
    pub date: NaiveDate,
    pub account: String,
    pub prompt_version: i32,
    pub summary: LegSummary,
    pub behavioral_tags: Vec<BehavioralTag>,
    pub leg_observations: Vec<LegObservation>,
    pub narrative_md: String,
    pub llm_call_id: Option<String>,
}
```

## Tasks

### Task 1: Migration

- [ ] Create `V14__day_reviews.sql` (paste schema above).
- [ ] `cargo test storage::migrations` — PASS.
- [ ] Commit: `feat(storage): V14 day_reviews table for structured trade reviews`.

### Task 2: BehavioralTag enum + weights (Rust)

**Files:** `services/trade_reviews/tags.rs`, `services/trade_reviews/mod.rs`, `services/mod.rs`

- [ ] **Step 1: Implement the closed enum**

```rust
//! `BehavioralTag` — closed enum the LLM picks from when authoring a
//! trade review. Mirrored 1:1 in `agent/trade_review.py`. A
//! mirror-test (`agent/tests/test_tag_mirror.py`) parses this file
//! and asserts the Python list matches name-for-name.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BehavioralTag {
    ChaseOwnExit,
    LateOtmLottery,
    GammaWindowViolation,
    SingleNameConcentration,
    PositionSizingUngraduated,
    PostLossRevenge,
    FlatClose,
    DisciplineOnLoser,
    ScaledInWinner,
    ScaledInLoser,
    ThesisMatchExecuted,
    OffThesisTrade,
}

impl BehavioralTag {
    /// Score weight applied during grade computation.
    pub fn weight(self) -> i32 {
        use BehavioralTag::*;
        match self {
            ChaseOwnExit => -10,
            LateOtmLottery => -10,
            GammaWindowViolation => -5,
            SingleNameConcentration => -5,
            PositionSizingUngraduated => -5,
            PostLossRevenge => -5,
            FlatClose => 5,
            DisciplineOnLoser => 5,
            ScaledInWinner => 3,
            ScaledInLoser => -7,
            ThesisMatchExecuted => 5,
            OffThesisTrade => -3,
        }
    }

    /// All values in declaration order. Used by the mirror-test and the
    /// LLM's tool schema.
    pub const ALL: [BehavioralTag; 12] = [
        BehavioralTag::ChaseOwnExit,
        BehavioralTag::LateOtmLottery,
        BehavioralTag::GammaWindowViolation,
        BehavioralTag::SingleNameConcentration,
        BehavioralTag::PositionSizingUngraduated,
        BehavioralTag::PostLossRevenge,
        BehavioralTag::FlatClose,
        BehavioralTag::DisciplineOnLoser,
        BehavioralTag::ScaledInWinner,
        BehavioralTag::ScaledInLoser,
        BehavioralTag::ThesisMatchExecuted,
        BehavioralTag::OffThesisTrade,
    ];
}
```

- [ ] **Step 2: Test weights are deterministic**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_close_is_positive() {
        assert!(BehavioralTag::FlatClose.weight() > 0);
    }

    #[test]
    fn chase_own_exit_is_strongly_negative() {
        assert!(BehavioralTag::ChaseOwnExit.weight() <= -10);
    }

    #[test]
    fn all_has_no_duplicates() {
        let mut sorted = BehavioralTag::ALL.iter().map(|t| format!("{:?}", t)).collect::<Vec<_>>();
        sorted.sort();
        let dedup_len = {
            let mut x = sorted.clone();
            x.dedup();
            x.len()
        };
        assert_eq!(dedup_len, sorted.len(), "ALL has duplicates: {:?}", sorted);
    }
}
```

- [ ] **Step 3: Commit**: `feat(trade_reviews): BehavioralTag enum + weights`.

### Task 3: `compute_grade` (deterministic)

**Files:** `services/trade_reviews/grade.rs`

- [ ] **Step 1: Failing test — same inputs same grade**

```rust
#[test]
fn same_inputs_yield_same_grade() {
    let summary = LegSummary { gross_pnl: 401.10, net_pnl: 380.0, commissions_total: 21.10, n_round_trips: 3, n_carryover: 0, win_rate: Some(2.0/3.0), by_symbol: Default::default() };
    let tags = vec![BehavioralTag::FlatClose, BehavioralTag::DisciplineOnLoser, BehavioralTag::ChaseOwnExit];
    let g1 = compute_grade(&summary, &tags);
    let g2 = compute_grade(&summary, &tags);
    assert_eq!(g1.grade, g2.grade);
    assert!((g1.score - g2.score).abs() < 1e-9);
}

#[test]
fn pure_winner_with_discipline_grades_at_least_b() {
    let summary = LegSummary { gross_pnl: 1500.0, net_pnl: 1200.0, commissions_total: 30.0, n_round_trips: 5, n_carryover: 0, win_rate: Some(0.9), by_symbol: Default::default() };
    let tags = vec![BehavioralTag::FlatClose, BehavioralTag::DisciplineOnLoser, BehavioralTag::ThesisMatchExecuted];
    let g = compute_grade(&summary, &tags);
    assert!(matches!(g.grade, GradeLetter::A | GradeLetter::B), "grade={:?}", g);
}

#[test]
fn loser_with_chase_grades_no_better_than_d() {
    let summary = LegSummary { gross_pnl: -200.0, net_pnl: -300.0, commissions_total: 100.0, n_round_trips: 4, n_carryover: 1, win_rate: Some(0.25), by_symbol: Default::default() };
    let tags = vec![BehavioralTag::ChaseOwnExit, BehavioralTag::LateOtmLottery, BehavioralTag::PostLossRevenge];
    let g = compute_grade(&summary, &tags);
    assert!(matches!(g.grade, GradeLetter::D | GradeLetter::F), "grade={:?}", g);
}
```

- [ ] **Step 2: Implement**

```rust
//! Pure deterministic grade computation. The LLM never picks the grade.

use serde::{Deserialize, Serialize};

use super::tags::BehavioralTag;
use super::types::LegSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum GradeLetter { A, B, C, D, F }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grade {
    pub grade: GradeLetter,
    pub score: f64,
}

/// `score = clamp(net_pnl / 100, -25, 25) + sum(tag_weights)`
/// Banding: ≥25 A; ≥10 B; ≥−5 C; ≥−20 D; else F.
pub fn compute_grade(summary: &LegSummary, tags: &[BehavioralTag]) -> Grade {
    let pnl_normalised = (summary.net_pnl / 100.0).clamp(-25.0, 25.0);
    let tag_score: i32 = tags.iter().map(|t| t.weight()).sum();
    let score = pnl_normalised + tag_score as f64;
    let grade = if score >= 25.0 {
        GradeLetter::A
    } else if score >= 10.0 {
        GradeLetter::B
    } else if score >= -5.0 {
        GradeLetter::C
    } else if score >= -20.0 {
        GradeLetter::D
    } else {
        GradeLetter::F
    };
    Grade { grade, score }
}
```

- [ ] **Step 3: Run, verify all three tests pass.**

- [ ] **Step 4: Add the determinism stress test**

```rust
#[test]
fn determinism_stress_1000_runs() {
    let summary = LegSummary { gross_pnl: 401.10, net_pnl: 380.0, commissions_total: 21.10, n_round_trips: 3, n_carryover: 0, win_rate: Some(2.0/3.0), by_symbol: Default::default() };
    let tags = vec![BehavioralTag::FlatClose, BehavioralTag::ChaseOwnExit];
    let baseline = compute_grade(&summary, &tags);
    for _ in 0..1000 {
        let g = compute_grade(&summary, &tags);
        assert_eq!(g.grade, baseline.grade);
        assert!((g.score - baseline.score).abs() < 1e-12);
    }
}
```

- [ ] **Step 5: Commit**: `feat(trade_reviews): deterministic compute_grade`.

### Task 4: `TradeReviewStore` — read + write

**Files:** `services/trade_reviews/store.rs`, `tests.rs`

- [ ] **Step 1: Failing test for idempotent UPSERT**

```rust
#[tokio::test]
async fn store_upserts_review_idempotently() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let req = sample_request();
    store.write(req.clone()).await.expect("first");
    store.write(req.clone()).await.expect("second");
    let row = store.read(req.date, &req.account, req.prompt_version).await.unwrap();
    assert!(row.is_some());
    let count: i64 = /* SELECT COUNT(*) FROM day_reviews via test helper */ 1;
    assert_eq!(count, 1, "expected 1 row, got {}", count);
}

#[tokio::test]
async fn store_separate_rows_per_prompt_version() {
    let (_tmp, db) = make_db();
    let store = TradeReviewStore::new(db);
    let mut req = sample_request();
    req.prompt_version = 1;
    store.write(req.clone()).await.unwrap();
    req.prompt_version = 2;
    store.write(req).await.unwrap();
    let v1 = store.read(/*date*/, /*acct*/, 1).await.unwrap();
    let v2 = store.read(/*date*/, /*acct*/, 2).await.unwrap();
    assert!(v1.is_some());
    assert!(v2.is_some());
}
```

- [ ] **Step 2: Implement `write` and `read`** (UPSERT pattern mirroring Phase 1's `ExecutionsStore::record`; serialize JSON via `serde_json::to_string` for the `behavioral_tags` and `leg_observations` columns).

- [ ] **Step 3: Run tests, commit.**

### Task 5: MCP read tool — `get_trade_review(date)`

**Files:** `mcp/tools/get_trade_review.rs`

- [ ] Mirror the shape of `mcp/tools/get_morning_pack.rs`. Args: `{date: String, account?: String, prompt_version?: i32}`. Returns `{date, account, review: {…}|null}`. Empty days return `{date, review: null}`, not an error.
- [ ] Read-only; no audit row.
- [ ] Wire into `mcp/handler.rs`.
- [ ] Tests: returns persisted row; absent ⇒ null envelope; invalid date ⇒ error; selects latest `prompt_version` when not specified.
- [ ] Commit: `feat(mcp): get_trade_review read tool`.

### Task 6: MCP write rail — `write_trade_review(...)`

**Files:** `mcp/tools/write_trade_review.rs`

- [ ] Mirror the shape of `mcp/tools/write_morning_pack.rs` (which IS audited via `mcp_audit`).
- [ ] Args: full `WriteTradeReviewRequest`. The tool calls `compute_grade(&summary, &tags)` server-side — the agent does NOT supply `grade`. The grade is computed deterministically from the (`summary`, `tags`) pair.
- [ ] Returns `{date, account, prompt_version, grade, score}` so the agent can log what was written.
- [ ] Audited: writes one `mcp_audit` row per call.
- [ ] Tests: writes a row; idempotent (second call with same key updates `generated_at` and overwrites narrative); audit row count = 1.
- [ ] Commit: `feat(mcp): write_trade_review agent-write rail`.

### Task 7: Python — `agent/trade_review.py` module

**Files:** `agent/trade_review.py`, `agent/prompts/trade_review.md`

- [ ] **Step 1: Mirror the BehavioralTag enum**

```python
"""Trade review module — mirrors the Rust BehavioralTag enum and provides
helpers for the EOD review's trade-review extension."""

from __future__ import annotations

from typing import Mapping, Sequence

# Mirror of services/trade_reviews/tags.rs::BehavioralTag.
# A mirror-test (tests/test_tag_mirror.py) pins this list against the Rust
# source. Don't add a value here without also adding it to Rust.
BEHAVIORAL_TAGS: tuple[str, ...] = (
    "chase_own_exit",
    "late_otm_lottery",
    "gamma_window_violation",
    "single_name_concentration",
    "position_sizing_ungraduated",
    "post_loss_revenge",
    "flat_close",
    "discipline_on_loser",
    "scaled_in_winner",
    "scaled_in_loser",
    "thesis_match_executed",
    "off_thesis_trade",
)


def leg_summary_from_legs(legs: Sequence[Mapping]) -> dict:
    """Compute the LegSummary the LLM consumes — server uses the same shape.

    Note: the server-side `write_trade_review` recomputes the grade from
    this summary + the chosen tags, so we don't need to send it. We send
    the summary so the LLM has accurate numbers in its prompt.
    """
    gross = sum(float(l.get("gross_pnl", 0.0)) for l in legs)
    commissions = sum(float(l.get("commission_total", 0.0)) for l in legs)
    net = sum(float(l.get("net_pnl", 0.0)) for l in legs)
    n_round = sum(1 for l in legs if "round_trip" in (l.get("tags") or []))
    n_carry = sum(1 for l in legs if "carryover" in (l.get("tags") or []))
    closed = [l for l in legs if "round_trip" in (l.get("tags") or [])]
    win_rate = (
        sum(1 for l in closed if float(l.get("net_pnl", 0.0)) > 0) / len(closed)
        if closed
        else None
    )
    by_symbol: dict[str, float] = {}
    for l in legs:
        sym = str(l.get("symbol", ""))
        by_symbol[sym] = by_symbol.get(sym, 0.0) + float(l.get("net_pnl", 0.0))
    return {
        "gross_pnl": gross,
        "net_pnl": net,
        "commissions_total": commissions,
        "n_round_trips": n_round,
        "n_carryover": n_carry,
        "win_rate": win_rate,
        "by_symbol": by_symbol,
    }


TRADE_REVIEW_TOOL_SCHEMA = {
    "name": "submit_trade_review",
    "description": "Pick behavioral tags and write a narrative scoring today's fills.",
    "input_schema": {
        "type": "object",
        "properties": {
            "behavioral_tags": {
                "type": "array",
                "items": {"type": "string", "enum": list(BEHAVIORAL_TAGS)},
                "description": "Closed enum — pick only from the listed values.",
            },
            "leg_observations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "leg_id": {"type": "string"},
                        "observation_md": {"type": "string"},
                        "tag": {"type": "string", "enum": list(BEHAVIORAL_TAGS)},
                    },
                    "required": ["leg_id", "observation_md"],
                },
            },
            "narrative_md": {"type": "string"},
        },
        "required": ["behavioral_tags", "narrative_md"],
    },
}
```

- [ ] **Step 2: Write `agent/prompts/trade_review.md`**

```markdown
You are an equity research analyst writing the structured "trade review" for one trader, after market close.

Inputs you receive:
- The day's leg-by-leg fills (already FIFO-matched, with realized P&L net of commissions).
- The agent's morning playbook (if any) so you can flag thesis matches and off-thesis trades.
- A LegSummary with the day's totals.

Your job:
1. Pick `behavioral_tags` from the closed enum. Apply each tag literally — don't tag `chase_own_exit` unless the trader actually re-entered the same instrument within 5 min of taking profit on it. Don't make tags up; the schema rejects unknown values.
2. Write `leg_observations` for the 1-3 most consequential legs of the day (the biggest winner, the biggest loser, and any legs that fired a behavioral tag). Each observation is 1-2 sentences. Tie back to the tag where applicable.
3. Write `narrative_md` — 200-400 words, markdown only. Cover (a) the day's net P&L and high-level shape, (b) what worked and why, (c) what didn't and why, (d) one or two notes on what to watch tomorrow. Don't issue a grade — the server computes it from your tags + the LegSummary.

Rules:
- Be honest. Credit good discipline; name bad behavior.
- Don't moralize. The trader is competent; you're a coach, not a parent.
- Don't speculate on intent. Stick to what the fills say.
- Don't include front-matter, fenced wrappers, or section headers above level 3.
```

- [ ] **Step 3: Commit**: `feat(agent): trade_review module + prompt`.

### Task 8: Python — extend `eod_review.py`

**Files:** `agent/eod_review.py`, `agent/mcp_client.py`

- [ ] **Step 1: Add MCP-client wrappers**

In `agent/mcp_client.py`, add helpers:

```python
async def get_trade_legs(self, *, date_iso: str, account: str | None = None, symbol: str | None = None) -> Any:
    args: dict = {"date": date_iso}
    if account is not None: args["account"] = account
    if symbol is not None: args["symbol"] = symbol
    return await self.call_tool("get_trade_legs", args)

async def write_trade_review(self, *, date_iso: str, account: str, prompt_version: int,
                              summary: dict, behavioral_tags: list[str],
                              leg_observations: list[dict], narrative_md: str,
                              llm_call_id: str | None = None) -> Any:
    args = {
        "date": date_iso, "account": account, "prompt_version": prompt_version,
        "summary": summary, "behavioral_tags": behavioral_tags,
        "leg_observations": leg_observations, "narrative_md": narrative_md,
    }
    if llm_call_id: args["llm_call_id"] = llm_call_id
    return await self.call_tool("write_trade_review", args)
```

- [ ] **Step 2: Extend `EodReviewMcp` protocol** with the new methods.

- [ ] **Step 3: Extend `run_eod_review`** to ALSO produce the trade review:

After the existing journal-entry write succeeds, add a second LLM call dedicated to the trade review:

```python
# (pseudocode — see eod_review.py for the actual orchestration shape)
legs_envelope = await mcp.get_trade_legs(date_iso=pack_iso, account=cfg.account)
legs = legs_envelope.get("legs", [])
if not legs:
    log.info("no fills on %s — skipping trade review", pack_iso)
    return result_so_far  # journal entry already written

summary = trade_review.leg_summary_from_legs(legs)

resp = await llm.call(
    model=cfg.models.smart,
    system=trade_review_system_prompt,
    messages=[{"role": "user", "content": format_trade_review_prompt(legs, summary, pack_ideas)}],
    tools=[trade_review.TRADE_REVIEW_TOOL_SCHEMA],
    tool_choice={"type": "tool", "name": "submit_trade_review"},
    max_tokens=2048,
)
tool_input = resp.tool_input  # parsed JSON from the forced tool call
await mcp.write_trade_review(
    date_iso=pack_iso, account=cfg.account, prompt_version=PROMPT_VERSION,
    summary=summary,
    behavioral_tags=tool_input["behavioral_tags"],
    leg_observations=tool_input.get("leg_observations", []),
    narrative_md=tool_input["narrative_md"],
)
```

- [ ] **Step 4: Add a `--no-trade-review` flag** for opt-out during smoke tests.

- [ ] **Step 5: Add `agent/tests/test_eod_review_trade_review.py`** with fakes for both MCP and LLM, asserting the trade review path: legs fetched, prompt assembled, forced-tool response parsed, `write_trade_review` called with expected args.

- [ ] **Step 6: Run tests** (`cd agent && uv run pytest tests/test_eod_review_trade_review.py`).

- [ ] **Step 7: Commit**: `feat(agent): eod_review writes structured trade reviews`.

### Task 9: Mirror-test (Python ↔ Rust enum)

**Files:** `agent/tests/test_tag_mirror.py`

- [ ] **Step 1: Write the test** — parse `src-tauri/src/services/trade_reviews/tags.rs`, extract the variant names, snake-case them, assert the resulting set equals `BEHAVIORAL_TAGS` from `agent/trade_review.py`.

```python
"""Mirror test — Rust BehavioralTag enum ↔ Python BEHAVIORAL_TAGS list.

If you add or remove a tag, BOTH sides must change, or this test fails."""

from __future__ import annotations

import re
from pathlib import Path

from trade_review import BEHAVIORAL_TAGS

RUST_FILE = (
    Path(__file__).resolve().parents[2]
    / "src-tauri" / "src" / "services" / "trade_reviews" / "tags.rs"
)


def _rust_variants() -> list[str]:
    text = RUST_FILE.read_text(encoding="utf-8")
    enum_block = re.search(
        r"pub enum BehavioralTag\s*\{([^}]+)\}", text, flags=re.DOTALL
    )
    assert enum_block, "BehavioralTag enum not found in tags.rs"
    raw_lines = enum_block.group(1).splitlines()
    variants: list[str] = []
    for line in raw_lines:
        line = line.strip().rstrip(",")
        if not line or line.startswith("//"):
            continue
        variants.append(line)
    return variants


def _to_snake(camel: str) -> str:
    s1 = re.sub("(.)([A-Z][a-z]+)", r"\1_\2", camel)
    return re.sub("([a-z0-9])([A-Z])", r"\1_\2", s1).lower()


def test_rust_and_python_tag_lists_match():
    rust = [_to_snake(v) for v in _rust_variants()]
    assert sorted(rust) == sorted(BEHAVIORAL_TAGS), (
        f"Rust = {sorted(rust)}\nPython = {sorted(BEHAVIORAL_TAGS)}"
    )
```

- [ ] **Step 2: Run, commit.**

## Exit criteria

- [ ] V14 migration applies cleanly.
- [ ] `compute_grade` is deterministic across 1000 runs (test asserts).
- [ ] `TradeReviewStore` UPSERTs idempotently and isolates per-prompt_version.
- [ ] `get_trade_review` returns persisted rows or null envelope; no audit.
- [ ] `write_trade_review` writes one audit row per call (mirror `write_morning_pack`).
- [ ] Mirror-test passes.
- [ ] Extended `eod_review.py` writes BOTH the existing journal entry AND a new `day_reviews` row when there are fills; gracefully skips the trade-review path when there are zero fills.
- [ ] Tracer-bullet (one trading day after deploy): at 17:00 ET cron, `get_trade_review(today)` returns a structured row with grade, behavioral_tags ⊆ enum, narrative_md non-empty.
- [ ] Update master Phase 4 row + this Status header.

## Gotchas

- **Server, not LLM, computes the grade.** This is the determinism guarantee. Don't add a `grade` field to `WriteTradeReviewRequest`. The MCP write rail computes it from `(summary, tags)` and stores both `grade` and `grade_score`.
- **`prompt_version` semantics.** Define a `PROMPT_VERSION = 1` constant in `agent/trade_review.py`. Bump when (a) the rubric weights change in Rust, (b) the tag enum gains/loses a value, OR (c) the system prompt in `agent/prompts/trade_review.md` changes materially. Old reviews stay queryable.
- **The journal entry doesn't go away.** Existing `eod_review.py` writes a markdown commentary; that path is untouched. The trade review is an ADDITIONAL output. Two independent consumer surfaces (UI journal page + structured profile dashboard) need both.
- **`leg_summary_from_legs` is a thin convenience.** The server's `write_trade_review` re-computes the summary from the database in v2 if dogfooding shows the LLM occasionally lies about the numbers. v1 trusts the agent.
- **Account selection.** v1 assumes a single account (looked up via `cfg.account`). If multi-account dogfooding becomes a thing, the agent loops once per managed account. Keep the table's PK including `account` so the path is open.
- **Empty days.** `get_trade_legs(today)` returns `{legs: [], totals: ...}` when there are no fills. The eod_review MUST detect this and skip the trade-review path (else the LLM gets a confusing prompt with zero legs).
- **Rate-limit on the LLM call.** The trade-review call uses ~1500 tokens in + 800 out; well under the per-loop USD cap. Still routed through `BudgetGuard` for the kill-switch.
- **Idempotency under retries.** If the agent crashes between the journal write and the trade-review write, the next cron run picks up only the missing piece — the journal append is already idempotent (existing behaviour); the trade-review write is keyed `(date, account, prompt_version)` so a re-run UPSERTs the same row.
- **Tag enum drift WILL happen.** v2 will add/remove tags. The mirror-test catches it at CI; the `prompt_version` bump catches it at runtime. Both layers are required.
