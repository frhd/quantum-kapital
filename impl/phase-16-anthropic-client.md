# Phase 16 — Anthropic client foundation + budget kill-switch

## Goal

A `LlmService` that talks to the Anthropic Messages API via plain `reqwest` (already in deps), supports prompt caching + tool-use forced JSON output, tracks token spend in `llm_calls`, and refuses to call when the daily budget is exhausted.

## Depends on

- [x] Phase 01 — `llm_calls` table.

## Out of scope

- Specific prompts (Phases 17–20).
- Streaming (we use blocking request/response; latency budget is fine for our cadence).
- Multi-provider abstraction. Anthropic only. If we ever swap, we wrap behind a trait then.

## Test plan (write tests FIRST)

`src-tauri/src/services/llm_service/tests.rs`. Use a `mockito` or simple `wiremock`-like local HTTP server (or a `reqwest` middleware test double) — pick one, keep it minimal.

- [x] `sends_correct_headers` — request includes `x-api-key`, `anthropic-version: 2023-06-01`, `content-type: application/json`.
- [x] `serializes_messages_correctly` — request body has `model`, `max_tokens`, `messages: [{role, content}]`, optional `system`, optional `tools`, optional `tool_choice`.
- [x] `parses_text_response` — mock returns `{content: [{type: "text", text: "..."}], usage: {...}}`; `LlmService::message(...)` returns `LlmResponse { text, tool_calls, usage }`.
- [x] `parses_tool_use_response` — mock returns `{content: [{type: "tool_use", name: "emit_thesis", input: {...}}]}`; `LlmService` returns the parsed input as `serde_json::Value`.
- [x] `forced_tool_use_returns_typed_args` — passing `tool_choice = ForceTool("emit_thesis")` results in `tool_calls.first()` matching schema.
- [x] `records_call_in_db_with_cost` — successful call inserts a row in `llm_calls` with `input_tokens`, `output_tokens`, `cache_read_tokens`, computed `cost_usd` based on model price table.
- [x] `cost_calculator_handles_each_supported_model` — sonnet-4-6, haiku-4-5 → correct $/M token math.
- [x] `prompt_cache_block_serializes_with_cache_control` — system prompt block with `cache: true` produces `{type: "text", text: "...", cache_control: {type: "ephemeral"}}`.
- [x] `daily_budget_kill_switch_blocks_new_calls` — sum of today's `cost_usd` ≥ `daily_llm_budget_usd` → `LlmService::message` returns `Err(LlmError::BudgetExhausted)` without making an HTTP request.
- [x] `kill_switch_resets_at_midnight_utc` — fixture clock past midnight; budget check uses today's date.
- [x] `propagates_4xx_errors` — mock returns 401 → `LlmError::Auth`.
- [x] `propagates_5xx_with_retry_disabled` — 500 → `LlmError::Upstream` after one attempt (no retries in this phase; can add backoff in a later iteration).

## Implementation tasks

- [x] Add to `AppConfig::api`:
  ```rust
  pub anthropic_api_key: Option<String>,
  pub daily_llm_budget_usd: f64, // default 5.0
  ```
  Loaded from env (`ANTHROPIC_API_KEY`) and persisted in `~/.config/quantum-kapital/settings.json`.
- [x] Update `.env.example` with `ANTHROPIC_API_KEY=`.
- [x] Create `src-tauri/src/services/llm_service/mod.rs`:
  ```rust
  pub struct LlmService { http: reqwest::Client, db: Arc<Db>, config: ... }
  pub struct LlmRequest {
      pub kind: LlmKind,             // Thesis | Decay | News | Ranker
      pub model: &'static str,        // claude-sonnet-4-6, claude-haiku-4-5
      pub max_tokens: u32,
      pub system: Vec<SystemBlock>,   // each block can have cache=true
      pub messages: Vec<Message>,
      pub tools: Option<Vec<ToolSchema>>,
      pub tool_choice: Option<ToolChoice>, // Auto | ForceTool(name)
      pub setup_id: Option<i64>,      // for ledger
  }
  pub struct LlmResponse {
      pub text: Option<String>,
      pub tool_calls: Vec<ToolCall>,
      pub usage: Usage,
  }
  impl LlmService {
      pub async fn message(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
      pub async fn cost_today_usd(&self) -> Result<f64>;
  }
  ```
- [x] `prices.rs` table:
  ```rust
  match model {
    "claude-sonnet-4-6" => (3.0, 15.0, 0.30),  // input, output, cache-read per M tokens
    "claude-haiku-4-5"  => (1.0,  5.0, 0.10),
    _ => return Err(LlmError::UnknownModel),
  }
  ```
  (Update with current pricing at implementation time; this is the structure.)
- [x] `LlmService` writes a row to `llm_calls` after every successful response.
- [x] Wire into `IbkrState` (`pub llm: Arc<LlmService>`).

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::llm_service` — green.
- [ ] Manual with real `ANTHROPIC_API_KEY`: small smoke-test command `tracker_llm_smoke_test` (debug-only, gated by `cfg(debug_assertions)`) that sends "hello" to Sonnet 4.6 and prints the response. Verify a `llm_calls` row appears.
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/llm_service/mod.rs`
- `src-tauri/src/services/llm_service/types.rs`
- `src-tauri/src/services/llm_service/prices.rs`
- `src-tauri/src/services/llm_service/tests.rs`

**Modified:**
- `src-tauri/Cargo.toml` — already has `reqwest`; may need a test-only HTTP-mock crate (e.g., `mockito`).
- `src-tauri/src/config/settings.rs`
- `src-tauri/.env.example`
- `src-tauri/src/ibkr/state.rs`
- `src-tauri/src/services/mod.rs`

## Scratchpad

- **Read** `impl/scratch/llm-prompts.md` for the model-choice table.
- **Write** any pricing changes or non-default request shapes there.

## Done when

`LlmService` makes real API calls, ledger reflects them, budget kill-switch blocks calls past `daily_llm_budget_usd`, smoke test passes.
