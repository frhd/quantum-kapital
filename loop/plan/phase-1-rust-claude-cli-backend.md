# Phase 1 — Rust `LlmBackend` trait + `ClaudeCliBackend` (subscription-backed inference)

> Part of [LLM backend split](master.md). See index for invariants.

**Status:** done (commit a44bdd2, 2026-05-03)

**Depends on:** none (foundation phase)

**Goal:** Introduce a higher-level `LlmBackend` trait above the existing
`AnthropicHttp` transport, ship a `ClaudeCliBackend` that spawns
`claude -p` for structured-output inference, and wire `lib.rs::run` to
pick the backend from `QK_LLM_BACKEND`. Default behavior is unchanged;
opting in flips all four Rust LLM call sites
(`news_interpreter`, `decay_watcher`, `thesis_generator`, `daily_ranker`)
onto the user's Claude subscription with one env var.

## Files

- New: `src-tauri/src/services/llm_service/cli_backend.rs` —
  `ClaudeCliBackend` impl. Builds the locked-down argv, spawns the
  subprocess via `tokio::process::Command`, parses the JSON envelope,
  produces `LlmResponse`. Includes the version-probe helper used at
  startup.
- New: `src-tauri/src/services/llm_service/backend.rs` — `LlmBackend`
  trait + `ApiBackend` (existing API path lifted out of `mod.rs` into a
  thin wrapper around `AnthropicHttp` + cost computation).
- Touches: `src-tauri/src/services/llm_service/mod.rs` — extract the
  pre-call (budget check) and post-call (parse + ledger write) logic so
  both backends share it. Add `LlmService::new_with_backend(backend,
  db, daily_budget_usd)`. Keep `LlmService::new(api_key, db,
  daily_budget_usd)` as a thin shim that constructs `ApiBackend` for
  back-compat. Add `LlmError::Backend { stage, message }` variant.
- Touches: `src-tauri/src/services/llm_service/types.rs` — add
  `LlmResponse::cost_usd_override: Option<f64>` (Some = parsed from CLI
  envelope; None = compute from tokens via `prices::cost_usd`). The new
  field is set only by `ClaudeCliBackend`; `ApiBackend` leaves it None
  to preserve byte-identical ledger writes for the default path.
- Touches: `src-tauri/src/services/llm_service/tests.rs` — keep all
  existing tests untouched (they exercise `AnthropicHttp` and the
  shared post-call path; both stay). Add new tests for argv
  construction, envelope parsing, the version probe, and the
  no-silent-fallback startup error.
- Touches: `src-tauri/src/lib.rs` — read `QK_LLM_BACKEND` once at
  startup; construct `ApiBackend` (default) or run the CLI version
  probe and construct `ClaudeCliBackend`; build `LlmService` from the
  chosen backend; emit one INFO log line stating which backend is
  active.
- Touches: `src-tauri/src/config/settings.rs` — add a parsed
  `llm_backend: LlmBackendKind` field alongside `anthropic_api_key`,
  populated from the env var. Existing settings-state shape
  unchanged otherwise.
- Touches: `src-tauri/.env.example` — document `QK_LLM_BACKEND` with
  `anthropic` (default) and `claude_cli`.
- Touches: `src-tauri/CLAUDE.md` — short note in the LLM-budget
  section that `cost_usd` is best-effort under `claude_cli`; tokens
  remain accurate; kill-switch still trips deterministically.

## CLI argv exposed

| Flag | Value | Why |
|---|---|---|
| `-p` | (presence) | Non-interactive print mode |
| `--output-format json` | literal | Single JSON envelope on stdout |
| `--model <name>` | from `LlmRequest.model` | Honor request's model selection |
| `--system-prompt <text>` | concatenated `LlmRequest.system` blocks | System prompt, no caching (CLI doesn't expose ephemeral cache breakpoints) |
| `--json-schema <schema>` | `tools[0].input_schema` when single forced tool | Structured output replaces `tool_use` |
| `--max-budget-usd <amt>` | `min($1.00, daily_budget_usd - cost_today_usd)` | Per-call hard cost cap mirrors the kill-switch |
| `--tools ""` | literal | Surveillance-only invariant: no built-in tools |
| `--strict-mcp-config` | (presence) | Don't inherit user's `~/.claude/settings.json` MCP entries |
| `--mcp-config '{}'` | literal | Empty MCP server set |
| `--permission-mode dontAsk` | literal | Non-interactive permission resolution |
| `--no-session-persistence` | (presence) | Don't write session state to disk |
| Stdin | concatenated `LlmRequest.messages` user/assistant text | Conversation history |

The `claude` binary is invoked positionally (no shell). Argv assembly is
unit-tested.

## Reuse (no new business logic this phase)

- The existing `AnthropicHttp` trait + `ReqwestAnthropicHttp` impl move
  unchanged into `ApiBackend` (just one level of indirection added).
- `prices::cost_usd` continues to compute USD from tokens; CLI mode
  uses it as the fallback when `total_cost_usd` is absent or zero.
- `utc_day_start_unix` + the `cost_today_usd` query stay where they are
  on `LlmService`. The kill-switch logic doesn't move.
- `LlmRequest` / `LlmResponse` shapes stay; only `cost_usd_override`
  is added.
- Existing tests in `tests.rs` are not modified; they continue to lock
  in API-path behavior.

## Decisions to make in this phase

- **Exact CLI envelope shape.** Run `claude -p --output-format json
  --json-schema '{"type":"object","properties":{"x":{"type":"string"}}}'
  hello` against the user's binary at implementation start; record
  the observed envelope (top-level keys, where `result`/`usage`/`cost`
  live) in a code comment at the top of `cli_backend.rs`. Pin parsing
  to those fields. If the envelope is incompatible with our needs
  (e.g. no usage), file in `QUESTIONS.md` and reassess.
- **Multi-turn conversation encoding.** First decision: do we ever
  send `LlmRequest.messages` with more than one entry? Audit the four
  Rust call sites. If all are single-turn (one user message), encode
  the user message via stdin and skip multi-turn complexity. If any
  are multi-turn, encode as a transcript fragment in the prompt body.
  Document the choice; v1 is allowed to be single-turn-only.
- **Caching breakpoint loss.** `LlmRequest.system` supports
  `cache: bool`; the CLI doesn't expose ephemeral-cache breakpoints.
  Decision: drop the cache flag silently in CLI mode (concatenate
  all blocks into one `--system-prompt`). Note in `master.md`'s
  observability caveat that cache savings don't apply under
  `claude_cli`.
- **Tool-choice handling for `Auto` and multi-tool.** Decision: the
  v1 CLI backend hard-errors with `LlmError::Backend` when
  `tool_choice == Auto` or `tools.len() > 1`. Document the
  restriction; today every Rust caller force-selects a single tool,
  so this is a no-op for the existing surface.
- **Working directory + env passed to the subprocess.** Decision:
  inherit nothing besides PATH and HOME (CLI needs HOME to find
  `~/.claude`). Strip `ANTHROPIC_*` from the child env so the CLI
  uses subscription auth, not the parent's API key. Working dir is
  a fixed empty `tempdir` so the CLI doesn't pick up a project
  CLAUDE.md by accident.
- **Probe timing + cache.** Decision: probe `claude --version` once
  at `LlmService::new_with_backend` construction; cache the version
  string on the service and emit it in the startup INFO line. Skip
  the probe when backend is `anthropic`.

## Exit criteria

- `QK_LLM_BACKEND=anthropic` (default) — every existing Rust test in
  `cargo test --manifest-path src-tauri/Cargo.toml` passes unchanged;
  no test is modified, only added. `cargo clippy -D warnings` is
  clean.
- `QK_LLM_BACKEND=claude_cli` — startup probe succeeds against the
  installed `claude` binary; `LlmService::new_with_backend` succeeds.
- Argv unit test asserts every always-on flag is present in the
  literal argv that `ClaudeCliBackend::call` would invoke for a
  representative `LlmRequest`. The same test asserts no
  `ANTHROPIC_*` env var leaks into the child env.
- Envelope-parsing unit test feeds a canned JSON envelope (recorded
  from a real `claude -p` invocation at implementation start) into
  `ClaudeCliBackend::parse_envelope` and asserts `LlmResponse.text`,
  `tool_calls[0].name`, `tool_calls[0].input`, and `usage.input_tokens`
  / `usage.output_tokens` match the expected values.
- No-silent-fallback test: constructing `LlmService` with
  `claude_cli` backend and a tampered `PATH` returns a clear
  startup error.
- End-to-end smoke (manual, recorded in PR): with
  `QK_LLM_BACKEND=claude_cli` set in `src-tauri/.env` and
  `ANTHROPIC_API_KEY` empty, `pnpm tauri dev` starts cleanly,
  `add_ticker AMD` fires the primer, `news_interpreter` writes a
  populated row to `news_cache` (no `ANTHROPIC_API_KEY is empty`
  WARN), and `llm_calls` has one new row with `kind="news"` and
  non-zero `output_tokens`.
- One startup INFO log line: `llm_service: backend=claude_cli
  version=2.1.126` (or `backend=anthropic-api`) appears exactly
  once per process start.
- File-size caps: `cli_backend.rs` and `backend.rs` each under the
  300-line soft cap. `mod.rs` stays under 500 hard cap (currently
  405 lines; refactor must net-shrink it).

## Gotchas

- **`pnpm tauri dev` env loading.** `scripts/tauri.sh` already passes
  `RUST_LOG`. `QK_LLM_BACKEND` must be loaded the same way
  `ANTHROPIC_API_KEY` is — i.e. read from `src-tauri/.env`. Verify
  the existing dotenv read covers it; if not, extend.
- **Test isolation.** Existing `tests.rs` constructs `LlmService`
  with a fake `AnthropicHttp`. The new `LlmBackend` trait must be
  carefully introduced so `MockHttp` continues to drive
  `ApiBackend` exactly as today; a regression here is the easiest
  way to break the seven other services that depend on
  `LlmService`.
- **`decay_watcher` test flake.** `respects_budget_kill_switch`
  panics with "MockHttp queue exhausted" pre-existing per the
  archived `loop/plan/done/ticker-intake-enrichment/QUESTIONS.md`
  Phase 1 entry. If you bump it during the refactor, fix it; do
  not paper over with `--no-verify`.
- **CLAUDE.md dirs leakage.** `claude` auto-discovers a project
  CLAUDE.md when run in a project directory. Working in
  `tempdir()` for each call is the simplest defense; verify with
  `--debug` once during implementation that no CLAUDE.md context
  leaks into the prompt.
- **Process leakage.** A timeout-killed subprocess must reap. Use
  `tokio::process::Child::start_kill` + `.wait()` on timeout to
  avoid zombies; integration test for the timeout path.
- **JSON schema escape hazard.** `--json-schema` takes a JSON
  string; if `tools[0].input_schema` contains shell metacharacters
  passing it via `Command::arg` (not shell) is fine. Argv-test
  guards against accidental shell wrapping.
- **`--max-budget-usd` floor.** When `daily_budget_usd -
  cost_today_usd` is near 0, do not pass a negative or zero value
  — short-circuit to `LlmError::BudgetExhausted` before invoking
  the subprocess (matches the existing kill-switch behavior; the
  CLI cap is a defense in depth).
- **Stdout vs stderr.** Per `bin/mcp-server.rs` precedent, stdout
  carries the protocol stream. Capture stdout for the JSON
  envelope; tee stderr at `tracing::debug` for diagnosis. Do not
  parse stderr.
- **`--bare` is the wrong move.** `--bare` forces strict
  `ANTHROPIC_API_KEY` auth, which is the opposite of the goal.
  Argv-test asserts `--bare` is *not* passed.
