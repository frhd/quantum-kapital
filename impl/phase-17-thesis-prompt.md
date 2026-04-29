# Phase 17 — Thesis prompt

## Goal

On each new `SetupCandidate`, generate a structured trade thesis via Claude Sonnet 4.6 (tool-use forced JSON), persist it on the `setups.thesis` column, re-emit the `SetupDetected` event with the populated thesis.

## Depends on

- [ ] Phase 16 — `LlmService` works.
- [ ] Phase 10 — `setups` rows exist.
- [ ] Phase 15 — events plumbing.

## Out of scope

- Decay-watcher (Phase 18).
- News interpretation (Phase 19).
- Daily ranker (Phase 20).

## Test plan (write tests FIRST)

`src-tauri/src/services/thesis_generator/tests.rs`.

- [ ] `builds_request_with_setup_data_and_context` — given a `SetupCandidate` + context (last 20 daily bars summary, fundamentals snapshot, recent news headlines), the generated `LlmRequest` has the right system prompt, the right tool schema, and `tool_choice = ForceTool("emit_thesis")`.
- [ ] `parses_tool_response_into_typed_thesis` — mock LLM returns `tool_calls = [{name: "emit_thesis", input: {thesis_md, conviction, invalidation_levels, risk_notes}}]`; generator returns a `Thesis` struct with those fields.
- [ ] `persists_thesis_to_setup_row` — generator writes `setups.thesis` (full JSON, not just markdown — or store markdown in `thesis` and the rest in a sibling JSON column; **decide and log in scratchpad**).
- [ ] `emits_setup_detected_with_thesis_after_generation` — re-emits `SetupDetected` with `thesis = Some(...)` so the frontend updates.
- [ ] `falls_back_gracefully_on_llm_error` — `LlmError::Upstream` → no crash; setup row remains with `thesis = None`; logs a warn.
- [ ] `falls_back_on_budget_exhausted` — `LlmError::BudgetExhausted` → same graceful behavior.
- [ ] `skips_when_thesis_already_present` — second invocation for the same setup_id is a no-op (idempotent).
- [ ] `system_prompt_uses_cache_control` — request has system block with `cache_control: {type: "ephemeral"}`.

## Implementation tasks

- [ ] Decide the storage shape for thesis output. **Recommendation:** add a `thesis_json TEXT` column to `setups` (additive ALTER) holding the full structured object; keep `thesis TEXT` for markdown-only convenience. Log in `schema-decisions.md`.
- [ ] Create `src-tauri/src/services/thesis_generator/mod.rs`:
  ```rust
  pub struct ThesisGenerator { llm, db, emitter }
  pub struct Thesis {
      pub thesis_md: String,
      pub conviction: char, // 'A' | 'B' | 'C'
      pub invalidation_levels: Vec<InvalidationLevel>,
      pub risk_notes: String,
  }
  impl ThesisGenerator {
      pub async fn generate(&self, setup_id: i64) -> Result<Thesis>;
  }
  ```
- [ ] System prompt (`prompts/thesis_v1.md`):
  ```
  You are a sober swing trader's analyst. You will receive structured signals — never narrate a chart you cannot see. Cite numeric `raw_signals`. Output ONLY through the `emit_thesis` tool.

  Style:
  - Concise, evidence-first.
  - Name the strategy and explain why the structured signals confirm or weaken it.
  - List concrete invalidation levels (price + reason).
  - Risk-flag anything unusual: low float, recent dilution, earnings-blackout window.
  ```
- [ ] Tool schema (`prompts/thesis_tool.json`):
  ```json
  { "name": "emit_thesis",
    "input_schema": { "type": "object",
      "properties": {
        "thesis_md": { "type": "string", "description": "Markdown thesis, 80–250 words" },
        "conviction": { "type": "string", "enum": ["A", "B", "C"] },
        "invalidation_levels": { "type": "array", "items": {
          "type": "object",
          "properties": { "label": {"type": "string"}, "price": {"type": "number"}, "reason": {"type": "string"} },
          "required": ["label", "price", "reason"]
        } },
        "risk_notes": { "type": "string" }
      },
      "required": ["thesis_md", "conviction", "invalidation_levels", "risk_notes"]
  } }
  ```
- [ ] User-message construction: include the `SetupCandidate` JSON + a 20-row bars summary (date, close, vol_ratio, daily_pct) + fundamentals essentials (P/E, market cap, latest revenue, latest EPS) + 3-5 most-relevant news headlines.
- [ ] Hook: `TrackerRunner::run_for` after persisting a setup, calls `thesis_generator.generate(setup.id)`. Emit re-fires `SetupDetected` with thesis.
- [ ] Front-end Watchlist row + setup-detected toast displays the markdown thesis on a tooltip / expandable card.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml services::thesis_generator` — green.
- [ ] Manual: `tracker_run_now('NVDA')` during a real setup; verify a `setups` row gains a populated `thesis_md` with structured invalidation levels; toast shows the thesis.
- [ ] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/thesis_generator/mod.rs`
- `src-tauri/src/services/thesis_generator/tests.rs`
- `src-tauri/src/services/llm_service/prompts/thesis_v1.md`
- `src-tauri/src/services/llm_service/prompts/thesis_tool.json`

**Modified:**
- `src-tauri/src/storage/schema.sql` (additive `thesis_json`)
- `src-tauri/src/services/tracker_runner.rs`
- `src-tauri/src/ibkr/types/tracker.rs` (`Setup.thesis_json` field)
- `src-tauri/src/ibkr/state.rs` (`pub thesis_generator`)
- `src/features/tracker/components/Watchlist.tsx` (display thesis)

## Scratchpad

- **Read / write** `impl/scratch/llm-prompts.md` Thesis section: prompt v1 content summary, model, observed token counts, cache-hit rate, quality notes.
- **Write** the decision about `thesis_json` column to `impl/scratch/schema-decisions.md`.

## Done when

A real detector hit produces a real LLM thesis with structured fields, cached system prompt drops costs on subsequent calls in the same batch, fallback behavior is graceful, frontend renders the thesis.
