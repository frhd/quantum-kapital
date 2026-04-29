# Phase 18 — Decay-watcher prompt

## Goal

For each `SetupActive` row, the intraday scheduler asks Haiku 4.5 every 5 min: "given the latest bars + thesis, is this still a valid setup?" Output is structured `{still_valid, reason, suggested_action}`. Replaces the stub from Phase 14.

## Depends on

- [x] Phase 14 — intraday scheduler invokes the decay-watcher.
- [x] Phase 16 — LlmService.
- [x] Phase 17 — thesis exists on `setups.thesis_json`.

## Out of scope

- Suggested-action execution (we never place orders). The `suggested_action` field is informational only.
- Multi-bar lookback summarization beyond a fixed window.

## Test plan (write tests FIRST)

`src-tauri/src/services/decay_watcher/tests.rs`.

- [x] `builds_request_with_thesis_and_recent_bars` — request includes the original `thesis_md` + invalidation_levels + last 12 intraday bars + current quote.
- [x] `forces_emit_decay_tool_use` — `tool_choice = ForceTool("emit_decay")`.
- [x] `parses_still_valid_true` — mock returns `{still_valid: true, reason: "structure intact"}`; watcher returns `DecayDecision { still_valid: true, ... }`.
- [x] `parses_still_valid_false_triggers_invalidation` — mock returns `{still_valid: false, reason: "broke below stop"}`; scheduler calls `state_machine.mark_invalidated(setup_id, reason)`.
- [x] `parses_target_hit_completes_setup` — mock returns `{still_valid: false, reason: "2R target reached", suggested_action: "scale_out"}`; mark_completed (not invalidated). Need to distinguish — schema includes `outcome: invalidated|target_hit|thesis_changed`.
- [x] `respects_budget_kill_switch` — `LlmError::BudgetExhausted` → returns `Ok(DecayDecision::skip())` so scheduler logs and continues.
- [x] `does_not_call_when_setup_too_fresh` — setup detected < 30 min ago → skip (avoids over-reacting to noise on first bars).
- [x] `caches_thesis_block_per_setup` — second call within 5 min for the same setup uses prompt cache (verify the request has `cache_control` on the thesis block).

## Implementation tasks

- [x] Replace `services/decay_watcher.rs` (the stub from Phase 14) with the real implementation backed by `LlmService`:
  ```rust
  pub struct DecayWatcher { llm: Arc<LlmService>, db: Arc<Db> }
  pub struct DecayDecision { pub still_valid: bool, pub outcome: DecayOutcome, pub reason: Option<String>, pub suggested_action: Option<String> }
  pub enum DecayOutcome { StillValid, Invalidated, TargetHit, ThesisChanged, Skipped }
  impl DecayWatcher {
      pub async fn check(&self, setup_id: i64) -> Result<DecayDecision>;
  }
  ```
- [x] System prompt (inlined as `SYSTEM_PROMPT`): "You watch a single trade setup. Given the original thesis and the most recent bars, decide if it's still valid. Output ONLY through the `emit_decay` tool. Be terse."
- [x] Tool schema (inlined in `tool_schema()`):
  ```json
  { "name": "emit_decay",
    "input_schema": { "type": "object",
      "properties": {
        "still_valid": {"type": "boolean"},
        "outcome": {"type": "string", "enum": ["still_valid","invalidated","target_hit","thesis_changed"]},
        "reason": {"type": "string"},
        "suggested_action": {"type": "string"}
      },
      "required": ["still_valid", "outcome", "reason"]
  } }
  ```
- [x] System block + thesis block both cached (`cache_control: ephemeral`). User block contains the freshly fetched bars.
- [x] Wire `DecayWatcher` into `IntradayScheduler` replacing the Phase 14 stub.
- [x] On outcome `Invalidated` / `ThesisChanged` → `state_machine.mark_invalidated(setup_id, reason)`.
- [x] On outcome `TargetHit` → `state_machine.mark_completed(setup_id)`.
- [x] On `StillValid` / `Skipped` → no state change.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::decay_watcher` — green.
- [ ] Manual: with one active setup, observe `tracing::info!` lines at each 5-min tick showing decay verdicts; force a price move below stop and verify the next tick invalidates.
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/llm_service/prompts/decay_v1.md`
- `src-tauri/src/services/llm_service/prompts/decay_tool.json`

**Modified:**
- `src-tauri/src/services/decay_watcher.rs` (replace stub with real impl + tests submodule)
- `src-tauri/src/services/intraday_scheduler.rs` (use real watcher; map outcomes to state-machine calls)
- `src-tauri/src/ibkr/state.rs` (`pub decay_watcher`)

## Scratchpad

- **Write** `impl/scratch/llm-prompts.md` Decay section with v1 prompt, observed costs (Haiku 4.5 = ~$0.005/call expected), cache hit rate.

## Done when

Active setups are re-evaluated every 5 min during RTH; invalidations / target-hits propagate through the state machine; budget kill-switch leaves scheduler running gracefully without LLM input.
