# Phase 5 — `playbooks` schema + extended `morning_sweep.py` + `get_today_playbook` / `write_playbook` MCP tools

> Part of [Behavioral assessment via MCP](master.md). See master for invariants.

**Status:** done (commit f892057, 2026-05-05)

**Depends on:** 3 (`get_watchlist_briefing` is the canonical input fan-out)

**Goal:** Persist a structured **playbook** every pre-market. The existing `agent/morning_sweep.py` already runs at 07:00 ET and writes a free-form `morning_pack` (`ranked_ideas`); this phase extends it to ALSO write structured `ranked_setups` (with trigger / entry / invalidation / target / conviction) and a `skip_list` via a new `write_playbook` MCP rail. The new `get_today_playbook(date)` read tool serves the cached playbook back to LLM clients and the desktop UI.

**Why this matters:** the morning pack today is research notes (free-form prose). The playbook is **orders-shaped** — every setup has a concrete trigger and invalidation level the trader can act on. This is what the user actually consumes pre-market; persisting it structured means the UI panel (Phase 7) and the LLM "what's the setup?" question both serve from the same cached row.

This phase mirrors Phase 4's structure: schema → service → two MCP tools (read + write rail) → agent extension → mirror discipline. The shapes are different but the patterns are identical, so this doc references Phase 4 explicitly where the pattern repeats.

## End-state for this phase

- `playbooks` SQLite table exists, keyed `(date, account, generation_id)`.
- `services/playbooks/` module with `types.rs`, `store.rs`, `tests.rs`.
- `mcp/tools/get_today_playbook.rs` — read tool, returns latest generation for date.
- `mcp/tools/write_playbook.rs` — write rail, audited.
- `agent/morning_sweep.py` extended:
  - After the existing `write_morning_pack` succeeds, runs a second LLM call with a forced tool that produces structured `ranked_setups` + `skip_list`.
  - Calls `write_playbook(date, account, generation_id, ranked_setups, skip_list, llm_call_id)`.
  - Continues to write the existing morning_pack as a sibling output (no removal).
- A schema-validation test pins the `RankedSetup` shape via serde round-trip.

## Files

**Create:**
- `src-tauri/src/storage/migrations/V15__playbooks.sql`
- `src-tauri/src/services/playbooks/mod.rs`
- `src-tauri/src/services/playbooks/types.rs`
- `src-tauri/src/services/playbooks/store.rs`
- `src-tauri/src/services/playbooks/tests.rs`
- `src-tauri/src/mcp/tools/get_today_playbook.rs`
- `src-tauri/src/mcp/tools/write_playbook.rs`
- `agent/playbook.py` — Python module: `RANKED_SETUPS_TOOL_SCHEMA`, helpers.
- `agent/prompts/playbook.md` — system prompt for the playbook LLM call.
- `agent/tests/test_playbook.py`

**Modify:**
- `src-tauri/src/services/mod.rs` — `pub mod playbooks;`
- `src-tauri/src/mcp/tools/mod.rs`, `src-tauri/src/mcp/handler.rs` — register new routers.
- `agent/morning_sweep.py` — extend `run_sweep` with playbook step.
- `agent/mcp_client.py` — add wrappers (`get_today_playbook`, `write_playbook`, `get_watchlist_briefing` if Phase 3 is also done).
- `agent/synthesizer.py` — keep; the playbook step is independent (separate LLM call) so morning_pack output shape is unchanged.

## Schema (V15__playbooks.sql)

```sql
-- V15__playbooks.sql
-- Structured playbook artifact. One playbook per (date, account, generation_id).
-- Multiple generation_ids per date allowed so an intraday refresh hook can be
-- added later without migration; v1 only writes one generation per day.

CREATE TABLE playbooks (
    date            TEXT    NOT NULL,        -- "YYYY-MM-DD" (ET)
    account         TEXT    NOT NULL,
    generation_id   INTEGER NOT NULL,        -- monotonic per (date, account)
    generated_at    TEXT    NOT NULL,        -- ISO 8601 UTC
    ranked_setups   TEXT    NOT NULL,        -- JSON array of RankedSetup
    skip_list       TEXT    NOT NULL,        -- JSON array of SkipEntry
    llm_call_id     TEXT,
    PRIMARY KEY (date, account, generation_id)
);

CREATE INDEX idx_playbooks_date ON playbooks(date);
CREATE INDEX idx_playbooks_account_date_gen
    ON playbooks(account, date DESC, generation_id DESC);
```

## End-state types (Rust)

```rust
// services/playbooks/types.rs
use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SetupBias { Long, Short }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Conviction { A, B, C }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceRef {
    pub source: String,        // "news" | "bars" | "setup" | "sentiment" | "fundamentals"
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RankedSetup {
    pub symbol: String,
    pub bias: SetupBias,
    /// Plain-English trigger: e.g. "reclaim of 5/4 HOD $175.29 on volume".
    pub trigger: String,
    /// Entry level / range: e.g. "$166" or "$165–166".
    pub entry: String,
    /// Invalidation level + condition: e.g. "lose $164 — gap-fill risk to $147".
    pub invalidation: String,
    pub target_1: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_2: Option<String>,
    pub conviction: Conviction,
    pub rationale_md: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkipEntry {
    pub symbol: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub date: NaiveDate,
    pub account: String,
    pub generation_id: i32,
    pub generated_at: DateTime<Utc>,
    pub ranked_setups: Vec<RankedSetup>,
    pub skip_list: Vec<SkipEntry>,
    pub llm_call_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WritePlaybookRequest {
    pub date: NaiveDate,
    pub account: String,
    pub ranked_setups: Vec<RankedSetup>,
    pub skip_list: Vec<SkipEntry>,
    pub llm_call_id: Option<String>,
}
```

## Tasks

### Task 1: Migration

- [ ] Create `V15__playbooks.sql`.
- [ ] `cargo test storage::migrations`.
- [ ] Commit: `feat(storage): V15 playbooks table for structured pre-market playbook`.

### Task 2: Types + store skeleton

**Files:** `services/playbooks/types.rs`, `mod.rs`, `services/mod.rs`

- [ ] Create types per the block above.
- [ ] Module root: `pub mod store; pub mod types; pub use store::PlaybookStore; pub use types::*;`
- [ ] Wire `pub mod playbooks;` into `services/mod.rs`.

### Task 3: PlaybookStore — read + write with auto-incrementing generation_id

**Files:** `services/playbooks/store.rs`, `tests.rs`

- [ ] **Step 1: Failing test for write returns a generation_id**

```rust
#[tokio::test]
async fn store_write_returns_monotonic_generation_id() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    let req = sample_request("2026-05-05", "U1");
    let g1 = store.write(req.clone()).await.unwrap();
    let g2 = store.write(req.clone()).await.unwrap();
    assert_eq!(g1, 1);
    assert_eq!(g2, 2);
}

#[tokio::test]
async fn store_read_latest_returns_most_recent_generation() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    store.write(sample_request("2026-05-05", "U1")).await.unwrap(); // generation 2
    let pb = store
        .read_latest(NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(), "U1")
        .await
        .unwrap()
        .expect("playbook");
    assert_eq!(pb.generation_id, 2);
}

#[tokio::test]
async fn store_read_specific_generation_returns_that_one() {
    let (_tmp, db) = make_db();
    let store = PlaybookStore::new(db);
    store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    store.write(sample_request("2026-05-05", "U1")).await.unwrap();
    let g1 = store
        .read_generation(NaiveDate::from_ymd_opt(2026, 5, 5).unwrap(), "U1", 1)
        .await
        .unwrap()
        .expect("g1");
    assert_eq!(g1.generation_id, 1);
}
```

- [ ] **Step 2: Implement** with auto-increment via `SELECT COALESCE(MAX(generation_id), 0) + 1 FROM playbooks WHERE date=? AND account=?` inside the same transaction.

- [ ] **Step 3: Run tests, commit.**

### Task 4: MCP read tool — `get_today_playbook(date, account?)`

**Files:** `mcp/tools/get_today_playbook.rs`

- [ ] Mirror `mcp/tools/get_morning_pack.rs`. Args: `{date: String, account?: String, generation_id?: i32}`. Returns `{date, account, playbook: {…}|null}`.
- [ ] When `generation_id` omitted, returns the latest generation for the date.
- [ ] Empty days return `{date, account, playbook: null}`.
- [ ] Read-only; no audit row.
- [ ] Tests + commit.

### Task 5: MCP write rail — `write_playbook(...)`

**Files:** `mcp/tools/write_playbook.rs`

- [ ] Mirror `write_morning_pack.rs` (audited).
- [ ] Args: `WritePlaybookRequest` (no `generation_id` — server assigns).
- [ ] Returns `{date, account, generation_id, n_setups, n_skip}`.
- [ ] Audited: writes one `mcp_audit` row per call.
- [ ] Schema-validation test: a serde round-trip `Playbook → JSON → Playbook` preserves all fields.
- [ ] Tests + commit.

### Task 6: Python — `agent/playbook.py` module + tool schema

**Files:** `agent/playbook.py`, `agent/prompts/playbook.md`

- [ ] **Step 1: Tool schema (mirrors RankedSetup / SkipEntry shape)**

```python
"""Playbook module — schemas + helpers for the morning sweep's
playbook extension."""

from __future__ import annotations

from typing import Mapping

# JSON schema for the LLM's forced tool call. The Rust write_playbook
# tool further validates the payload at the MCP boundary, so any drift
# fails loudly rather than silently storing junk.
SETUP_BIAS = ("long", "short")
CONVICTION = ("A", "B", "C")

RANKED_SETUPS_TOOL_SCHEMA = {
    "name": "submit_playbook",
    "description": "Emit ranked, actionable setups for today plus an explicit skip list.",
    "input_schema": {
        "type": "object",
        "properties": {
            "ranked_setups": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string"},
                        "bias": {"type": "string", "enum": list(SETUP_BIAS)},
                        "trigger": {"type": "string"},
                        "entry": {"type": "string"},
                        "invalidation": {"type": "string"},
                        "target_1": {"type": "string"},
                        "target_2": {"type": "string"},
                        "conviction": {"type": "string", "enum": list(CONVICTION)},
                        "rationale_md": {"type": "string"},
                        "evidence_refs": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "source": {"type": "string"},
                                    "note": {"type": "string"},
                                },
                                "required": ["source", "note"],
                            },
                        },
                    },
                    "required": [
                        "symbol", "bias", "trigger", "entry", "invalidation",
                        "target_1", "conviction", "rationale_md",
                    ],
                },
            },
            "skip_list": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string"},
                        "reason": {"type": "string"},
                    },
                    "required": ["symbol", "reason"],
                },
            },
        },
        "required": ["ranked_setups", "skip_list"],
    },
}
```

- [ ] **Step 2: System prompt — `agent/prompts/playbook.md`**

```markdown
You are an equity desk strategist writing a tight, actionable pre-market playbook for one trader.

Inputs you receive:
- A composite briefing for every watchlist symbol: quote, recent daily bars, news, sentiment, active setups, fundamentals.
- The trader's profile (if available): tag frequencies, P&L by tag, recent behavioral incidents from the last 7-30 day_reviews.

Your job:
1. Produce `ranked_setups` — a list of A/B/C-conviction setups. Each setup MUST have:
   - `bias` (`long` or `short`)
   - `trigger` — a precise, observable price/volume condition (e.g. "reclaim of 5/4 HOD $175.29 on volume > 5-day avg")
   - `entry` — the level or range to enter (e.g. "$166" or "$165–166")
   - `invalidation` — the level + condition that voids the setup (e.g. "lose $164 — gap-fill risk to $147")
   - `target_1` — first profit target
   - `target_2` (optional) — extension target
   - `rationale_md` — 2-4 sentences on WHY (catalyst, levels, R:R)
   - `evidence_refs` — pointers to specific data items in the briefing (`{source, note}`)

2. Produce `skip_list` — explicitly named symbols to AVOID today, with reasons. Use this when:
   - The trader has a recent behavioral incident on that name (e.g. `chase_own_exit` 3+ times last 7d ⇒ deprioritize TSLA 0DTE).
   - The setup is event-locked (e.g. earnings AMC tonight) and not tradeable.
   - The chart shape is distributing or the catalyst is exhausted.

3. Be honest about no-trade days. If nothing meets the bar, return `ranked_setups: []` and explain in skip_list entries.

Rules:
- Don't invent data. If `evidence_refs` would have to be made up, drop the setup.
- A-conviction is rare. B is most common. C is "watch only".
- One name per `ranked_setups` entry; no spreads in v1.
- The rationale must be defensible at a desk meeting tomorrow.
- Write rationale as markdown but keep it under 4 sentences per setup.
```

- [ ] **Step 3: Commit**: `feat(agent): playbook module + prompt`.

### Task 7: Python — extend `morning_sweep.py`

**Files:** `agent/morning_sweep.py`, `agent/mcp_client.py`

- [ ] **Step 1: Add MCP-client wrappers** for `get_today_playbook`, `write_playbook`, and (if Phase 3 is in) `get_watchlist_briefing`.

- [ ] **Step 2: Extend `run_sweep`** with a playbook step AFTER `write_morning_pack` succeeds:

```python
# After morning_pack is written, run the playbook generator on the same
# bundles. This is a second LLM call dedicated to producing structured
# actionable setups; the morning_pack remains the prose-research surface.

playbook_resp = await llm.call(
    model=cfg.models.smart,
    system=playbook_system_prompt,
    messages=[{"role": "user", "content": format_playbook_prompt(bundles, trader_profile)}],
    tools=[playbook.RANKED_SETUPS_TOOL_SCHEMA],
    tool_choice={"type": "tool", "name": "submit_playbook"},
    max_tokens=3000,
)
guard.record(cfg.models.smart, playbook_resp.input_tokens, playbook_resp.output_tokens, envelope_cost_usd=playbook_resp.cost_usd)
tool_input = playbook_resp.tool_input
await mcp.write_playbook(
    date_iso=iso_today, account=cfg.account,
    ranked_setups=tool_input["ranked_setups"],
    skip_list=tool_input["skip_list"],
)
```

- [ ] **Step 3: `format_playbook_prompt(bundles, trader_profile)`** assembles a compact prompt body. v1 passes `trader_profile = None` (Phase 6 wires the real one). The prompt template MUST include a placeholder for the profile so adding it later is a one-line change.

- [ ] **Step 4: `--no-playbook` flag** for opt-out smoke tests.

- [ ] **Step 5: Add `agent/tests/test_morning_sweep_playbook.py`** asserting the playbook step is invoked, the forced tool response is parsed, and `write_playbook` is called with the right args.

- [ ] **Step 6: Run agent tests** (`uv run pytest`).

- [ ] **Step 7: Commit**: `feat(agent): morning_sweep writes structured playbooks`.

### Task 8: Schema round-trip test (Rust ↔ JSON)

**Files:** `services/playbooks/tests.rs`

- [ ] Write a serde round-trip test: build a `Playbook` value with all fields populated; serialize to JSON; parse back; assert equality. Pin against accidental field renames.

- [ ] Commit.

## Exit criteria

- [ ] V15 migration applies cleanly.
- [ ] `PlaybookStore::write` returns monotonic `generation_id` per `(date, account)`.
- [ ] `read_latest` and `read_generation` both work.
- [ ] `get_today_playbook` returns latest generation or null envelope; no audit.
- [ ] `write_playbook` writes one audit row per call.
- [ ] Serde round-trip test passes.
- [ ] Extended `morning_sweep.py` writes BOTH the existing morning_pack AND a new `playbooks` row at every cron tick; gracefully skips the playbook step if budget exhausted.
- [ ] Tracer-bullet (next trading day after deploy): at 07:00 ET cron, `get_today_playbook(today)` returns a row with `ranked_setups[]` (each with trigger/entry/invalidation/target_1/conviction) + `skip_list[]`.
- [ ] Update master Phase 5 row + this Status header.

## Gotchas

- **Two LLM calls per morning, not one.** The morning_pack and the playbook are separate LLM calls because the prompts and forced-tool schemas differ. v2 may merge if the cost/latency demands; v1 keeps them independent for clarity. Both routed through `BudgetGuard`.
- **`generation_id` semantics.** v1 always writes generation_id=N where N is the next-after-MAX. So the first morning of a date is generation 1, and an intraday refresh hook (v2) would push generation 2. Don't reset to 0 across dates — the PK includes `date` so 1's are distinct per day.
- **Empty `ranked_setups[]` is OK.** If the LLM concludes nothing meets the bar, the playbook still ships with an empty `ranked_setups` and a populated `skip_list` explaining why. The UI panel's empty-state shows "no A/B-conviction setups today; here's the skip list."
- **`evidence_refs` are not enforced.** v1 stores them as freeform `{source, note}`. v2 may validate `source` against an enum if dogfooding shows the LLM uses inconsistent labels.
- **Schema drift.** Bumping a field in `RankedSetup` requires (a) Rust `types.rs`, (b) Python `RANKED_SETUPS_TOOL_SCHEMA`, (c) the round-trip test fixture. Three places. Worth a checklist comment at the top of each file.
- **Playbook freshness.** v1 writes one playbook per cron tick (07:00 ET). The schema admits multiple generations per date, but no intraday trigger ships in v1. Phase 6's behavioral feedback runs at the same 07:00 cron — not intraday.
- **Cost.** Empirically a `~3000 max_tokens` playbook run costs <$0.05 against the mid-tier model. Well under daily budget.
- **`format_playbook_prompt` must be deterministic.** Given the same bundles + profile, it should produce the same string. This makes LLM-call replay possible during debugging.
- **Trader profile placeholder.** v1 passes `trader_profile = None`. Phase 6 wires the real profile in. The prompt template MUST already include a `{trader_profile_section}` placeholder so the wire-up is purely a Python edit, not a prompt edit.
- **Account scoping.** Same as Phase 4 — v1 single-account; the table PK includes `account` so multi-account is unblocked.
