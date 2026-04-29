# Phase 17 — Thesis prompt

## Goal

On each new `SetupCandidate`, generate a structured trade thesis via Claude Sonnet 4.6 (tool-use forced JSON), persist it on the `setups.thesis` column, re-emit the `SetupDetected` event with the populated thesis.

## Depends on

- [x] Phase 16 — `LlmService` works.
- [x] Phase 10 — `setups` rows exist.
- [x] Phase 15 — events plumbing.

## Out of scope

- Decay-watcher (Phase 18).
- News interpretation (Phase 19).
- Daily ranker (Phase 20).

## Test plan (write tests FIRST)

`src-tauri/src/services/thesis_generator/tests.rs`.

- [x] `builds_request_with_setup_data_and_context` — given a `SetupCandidate` + context (last 20 daily bars summary, fundamentals snapshot, recent news headlines), the generated `LlmRequest` has the right system prompt, the right tool schema, and `tool_choice = ForceTool("emit_thesis")`.
- [x] `parses_tool_response_into_typed_thesis` — mock LLM returns `tool_calls = [{name: "emit_thesis", input: {thesis_md, conviction, invalidation_levels, risk_notes}}]`; generator returns a `Thesis` struct with those fields.
- [x] `persists_thesis_to_setup_row` — generator writes `setups.thesis` (full JSON, not just markdown — or store markdown in `thesis` and the rest in a sibling JSON column; **decide and log in scratchpad**).
- [x] `emits_setup_detected_with_thesis_after_generation` — re-emits `SetupDetected` with `thesis = Some(...)` so the frontend updates.
- [x] `falls_back_gracefully_on_llm_error` — `LlmError::Upstream` → no crash; setup row remains with `thesis = None`; logs a warn.
- [x] `falls_back_on_budget_exhausted` — `LlmError::BudgetExhausted` → same graceful behavior.
- [x] `skips_when_thesis_already_present` — second invocation for the same setup_id is a no-op (idempotent).
- [x] `system_prompt_uses_cache_control` — request has system block with `cache_control: {type: "ephemeral"}`.

## Implementation tasks

- [x] Decide the storage shape for thesis output. **Decision:** added `thesis_json TEXT` column (additive ALTER via `add_column_if_missing`) holding the full structured `Thesis` object; kept `thesis TEXT` for markdown-only convenience. Logged in `impl/scratch/schema-decisions.md`.
- [x] Create `src-tauri/src/services/thesis_generator/mod.rs`:
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
- [x] System prompt (inlined as `SYSTEM_PROMPT` const in `thesis_generator/mod.rs` rather than a separate `prompts/thesis_v1.md` — single-source-of-truth, picked up via the `SYSTEM_PROMPT` constant; bumping versions = updating the const + `impl/scratch/llm-prompts.md` entry):
  ```
  You are a sober swing trader's analyst. You will receive structured signals — never narrate a chart you cannot see. Cite numeric `raw_signals`. Output ONLY through the `emit_thesis` tool.

  Style:
  - Concise, evidence-first.
  - Name the strategy and explain why the structured signals confirm or weaken it.
  - List concrete invalidation levels (price + reason).
  - Risk-flag anything unusual: low float, recent dilution, earnings-blackout window.
  ```
- [x] Tool schema (inlined as `tool_schema()` helper in `thesis_generator/mod.rs`, returning `serde_json::json!({...})`; equivalent JSON shape to:
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
- [x] User-message construction: include the `Setup` row JSON + a 20-row bars summary (`time, close, volume, daily_pct`) + ≤ 5 most-relevant news headlines. Fundamentals essentials are deferred until `TrackerRunner::context_for` actually fetches them (no current detector reads them, so injecting them just to feed the LLM was out of scope for the v1 wiring).
- [x] Hook: `TrackerRunner::run_for` after persisting a setup calls `thesis_generator.generate(&setup, &thesis_ctx)` (chained on at construction time via `.with_thesis_generator(...)`). On success the generator owns the `SetupDetected { thesis: Some(md) }` emit; on graceful fallback / skip, the runner emits the Phase 15 `thesis: None` event so the watchlist still updates.
- [x] Front-end Watchlist row + setup-detected toast displays the markdown thesis: `Watchlist.tsx` renders a truncated preview line under the `SetupBadge` (full markdown via `title` tooltip); `TrackerTab.tsx` toast description uses the markdown body when present, falls back to `direction @ trigger — thesis pending` while the LLM call is in flight; dedup key is `${setup.id}:${thesisMd ? "thesis" : "pending"}` so both events fire toasts.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::thesis_generator` — green (8 unit tests + 1 runner integration test).
- [ ] Manual: `tracker_run_now('NVDA')` during a real setup; verify a `setups` row gains a populated `thesis_md` with structured invalidation levels; toast shows the thesis. *(Pending real ANTHROPIC_API_KEY walk-through.)*
- [x] `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --check` clean; `pnpm typecheck` + `pnpm lint` (0 errors) + `pnpm exec prettier --check` on touched files clean.

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
