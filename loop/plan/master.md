# LLM backend split → run inference on the local Claude Code subscription instead of `ANTHROPIC_API_KEY`: ~2 weeks

## Context

Today every LLM call site in the repo requires an Anthropic API key.

- **Rust**: `LlmService::message` (`src-tauri/src/services/llm_service/mod.rs:205`)
  is the single entrypoint. It posts to `https://api.anthropic.com/v1/messages`
  via the `AnthropicHttp` trait (`mod.rs:35`), parses the response, computes
  cost from `prices::cost_usd`, and writes one row to `llm_calls`. If
  `ANTHROPIC_API_KEY` is empty, the call returns `LlmError::NoApiKey` (the
  exact warning we just saw in `/tmp/qk-tauri.log`:
  `news interpreter LLM call failed gracefully: ANTHROPIC_API_KEY is empty
  symbol="AMD"`).
- **Python**: `agent/llm.py::AnthropicLlmClient` wraps `AsyncAnthropic` and
  raises at construction if `ANTHROPIC_API_KEY` is unset. The three agent
  loops (`morning_sweep`, `alert_dive`, `ticker_intake`) all depend on it.

The user has Claude Code installed (`/home/farhad/.local/bin/claude` v2.1.126)
authenticated against their Pro/Max subscription. The CLI in v2.1+ exposes
exactly what we need to run as a structured-output backend:

- `-p` / `--print` — non-interactive
- `--output-format json` — single JSON envelope
- `--json-schema <schema>` — enforced structured output (replaces `tool_use`)
- `--system-prompt <prompt>` — system prompt
- `--model <name>` — model selection (accepts `claude-sonnet-4-6`)
- `--tools ""` — disable all built-in tools (lockdown)
- `--strict-mcp-config --mcp-config '{}'` — empty MCP surface (no recursion)
- `--no-session-persistence` — don't save session state
- `--max-budget-usd <amount>` — per-call hard cost cap
- `--permission-mode dontAsk` — non-interactive permission resolution

**Inversion.** Today the LLM client is an HTTPS POST that needs a key. End
state: it is **a subprocess call to `claude -p`** that uses the user's
existing subscription auth. The Anthropic API path is preserved as the
default for backwards compatibility, but a single env var flips both Rust
and Python over to subscription-backed inference.

## End-state architecture

| Subsystem | Responsibility |
|---|---|
| **`LlmBackend` trait** (new, above the existing `AnthropicHttp` trait in `src-tauri/src/services/llm_service/mod.rs`) | Higher-level seam: takes an `LlmRequest`, returns an `LlmResponse`. Two impls: `ApiBackend` (wraps the existing `AnthropicHttp` path; current behavior) and `ClaudeCliBackend` (new; spawns `claude -p`). |
| **`ClaudeCliBackend`** (new, `src-tauri/src/services/llm_service/cli_backend.rs`) | Spawns the locked-down `claude -p ...` subprocess, marshals `LlmRequest` → CLI args + stdin, parses the JSON envelope into `LlmResponse`. When `req.tools` + `tool_choice=ForceTool(name)` are set, encodes the single tool's `input_schema` as `--json-schema` and synthesizes a `ToolCall { name, input }` from the JSON result. |
| **`LlmService::new_with_backend`** (new constructor; existing `LlmService::new` stays as a thin wrapper for the API backend) | Picks the backend at construction time. `lib.rs::run` reads `QK_LLM_BACKEND` and passes the right backend in. The `message()` method is unchanged from the caller's view. |
| **Python `LlmClient` Protocol** (existing, `agent/llm.py:34`) | Unchanged. New impl `ClaudeCliLlmClient` lives next to `AnthropicLlmClient` in the same file (or its own module if size demands it). Three loops pick which to use via `agent/config.py` reading the same `QK_LLM_BACKEND` env var. |
| **`llm_calls` ledger** | Continues to be the budget source of truth. CLI mode parses `total_cost_usd` from the envelope when present; falls back to `prices::cost_usd(model, input_tokens, output_tokens, 0)` using the reported usage. Tokens are accurate; the USD figure is best-effort under subscription pricing. |
| **`BudgetGuard` (Python)** | Unchanged. Tracks per-loop USD spend the same way; CLI mode parses cost from the envelope. |
| **Audit** | `LlmService` startup logs one INFO line: `llm_service: backend = anthropic-api | claude-cli`. No new MCP tools. No change to `mcp_audit`. |

## Hard invariants

1. **Surveillance-only stays.** The CLI backend always passes `--tools ""`,
   `--strict-mcp-config`, `--mcp-config '{}'`, and `--permission-mode dontAsk`.
   No code path may construct a `claude` invocation without these flags. CI
   greps assert this in both phases.
2. **All LLM calls stay funneled through `LlmService` (Rust) or `LlmClient`
   (Python).** No call site shells out to `claude` directly. The CLI is a
   transport detail of the existing seams, not a new seam.
3. **Default behavior is byte-identical to today.** When `QK_LLM_BACKEND`
   is unset or set to `anthropic`, the code path is the existing
   `ReqwestAnthropicHttp` / `AsyncAnthropic` path. Any divergence is a bug.
4. **Trait seams unchanged for tests.** Tests still mock at `AnthropicHttp`
   (existing) or the new `LlmBackend` (one level higher). No test ever
   spawns a real `claude` subprocess.
5. **Ledger integrity.** Every successful CLI call writes one `llm_calls`
   row with the same columns the API path writes. `cost_usd` is parsed from
   the CLI envelope when available; otherwise computed from reported tokens
   via `prices::cost_usd`; otherwise 0 with a WARN log. Tokens are always
   accurate when the envelope reports them.
6. **No silent fallback.** If `QK_LLM_BACKEND=claude_cli` is set and the
   `claude` binary is missing or returns non-zero on a probe, fail fast at
   startup with a clear error. Do not silently fall back to the API path.
7. **Recursion prevention is unconditional.** The `--mcp-config '{}'`
   + `--strict-mcp-config` pair keeps a CLI sub-instance from inheriting
   the parent's MCP config and re-entering the app's socket at
   `~/.local/share/com.quantyc.qqk/mcp.sock`.
8. **Pre-commit sacred.** `cargo fmt --check`, `cargo clippy -D warnings`,
   `prettier --check`, `eslint`, `uv run pytest`. Never `--no-verify`.
9. **File-size caps respected.** Rust soft 300 / hard 500
   (`CONTRIBUTING.md`). Python the same convention as `agent/llm.py`. Past
   the hard cap requires `// allow-large-file:` justifier.

Violating the letter of these rules is violating the spirit.

## Defaults committed (overridable per-phase)

- **Backend selection:** `QK_LLM_BACKEND` env var, values `anthropic`
  (default) | `claude_cli`. Same var read by both Rust and Python so
  `pnpm tauri dev` and `uv run qk-ticker-intake` behave consistently.
- **Subprocess timeout:** 60s per call, both languages. CLI startup adds
  ~hundreds of ms; 60s leaves room for slow first-token + structured-output
  generation on Sonnet.
- **Always-on CLI flags:** `-p`, `--output-format json`,
  `--no-session-persistence`, `--strict-mcp-config`, `--mcp-config '{}'`,
  `--permission-mode dontAsk`, `--tools ""`. Never pass `--bare` (we want
  subscription auth, not strict API key).
- **Per-call budget cap:** `--max-budget-usd $1.00` clamped against
  `daily_budget_usd - cost_today_usd`. CLI-side hard cap mirrors the
  existing kill-switch.
- **Models:** `claude-sonnet-4-6` and `claude-haiku-4-5` — the same set the
  pricing table supports. CLI accepts them as `--model` aliases or full
  names.
- **Tool calls in CLI mode:** only the single-tool / `ForceTool` shape is
  supported (which is what every current Rust call site uses — Ranker,
  DecayWatcher, ThesisGenerator, NewsInterpreter all force one tool).
  `tool_choice=Auto` and multi-tool requests fall back to a runtime error
  in CLI mode for v1; revisit if a call site needs it.
- **Tool input → CLI:** when a single forced tool is present, its
  `input_schema` becomes `--json-schema <schema>`; the CLI's JSON
  result is wrapped into a synthetic `ToolCall { name, input }`.
- **Failure surface:** subprocess failures map to a new `LlmError::Backend`
  variant (Rust) and a `BackendError` exception (Python). Existing graceful
  degraders (e.g. `NewsInterpreter`) treat it like any other LLM error.
- **Audit:** one startup INFO line per process: `llm: backend=<name>`.
  No new MCP tool, no new audit row.

## Phase index

| Phase | File | Depends on | Status |
|---|---|---|---|
| 1. Rust `LlmBackend` trait + `ClaudeCliBackend` | [phase-1-rust-claude-cli-backend.md](phase-1-rust-claude-cli-backend.md) | — | done (commit a44bdd2, 2026-05-03) |
| 2. Python `ClaudeCliLlmClient` + agent loop wiring | [phase-2-python-claude-cli-backend.md](phase-2-python-claude-cli-backend.md) | 1 | todo |

> **Status convention:** `todo` | `in-progress (started YYYY-MM-DD)` | `done (commit <sha>, YYYY-MM-DD)`. Update both this table AND the phase file's `**Status:**` header at phase start and exit. Don't start a phase whose dependencies aren't `done`.

## Critical files

| Concern | Path |
|---|---|
| Rust LlmService entrypoint + transport seam | `src-tauri/src/services/llm_service/mod.rs` |
| Rust LlmRequest / LlmResponse / Usage types | `src-tauri/src/services/llm_service/types.rs` |
| Rust pricing table | `src-tauri/src/services/llm_service/prices.rs` |
| Rust LlmService tests + fakes | `src-tauri/src/services/llm_service/tests.rs` |
| Service composition (where backend is chosen) | `src-tauri/src/lib.rs` |
| Settings / env var reads | `src-tauri/src/config/settings.rs` |
| LLM call sites (Rust) | `src-tauri/src/services/news_interpreter/`, `src-tauri/src/services/decay_watcher/`, `src-tauri/src/services/thesis_generator/`, `src-tauri/src/services/daily_ranker/` |
| `llm_calls` ledger schema | `src-tauri/src/storage/migrations/V01__baseline.sql`, `V08__predictions_and_attribution.sql` |
| MCP budget tool | `src-tauri/src/mcp/tools/budget.rs` |
| Python LLM client + Protocol | `agent/llm.py` |
| Python budget guard | `agent/budget_guard.py` |
| Python config (env var read) | `agent/config.py`, `agent/config.toml` |
| Python loops that consume the client | `agent/morning_sweep.py`, `agent/alert_dive.py`, `agent/ticker_intake.py` |
| Python tests + fakes | `agent/tests/` |
| Agent runtime + install docs | `agent/README.md` |
| `.env.example` for the `QK_LLM_BACKEND` doc | `src-tauri/.env.example` |
| Repo-level rules | `CLAUDE.md`, `src-tauri/CLAUDE.md`, `agent/README.md` |

## Sequencing + cadence

- **W1:** Phase 1. Rust `LlmBackend` trait introduced; `ClaudeCliBackend`
  added; `lib.rs::run` reads `QK_LLM_BACKEND` and constructs the chosen
  backend. Visible win: with the env var set in `src-tauri/.env`, the AMD
  log line we just observed turns from a graceful-fail WARN into a
  populated `news_cache` interpretation row, no API key required. All four
  Rust LLM call sites benefit transparently.
- **W2:** Phase 2. Python `ClaudeCliLlmClient` added; loops read the same
  env var. Visible win: morning sweep, alert dive, and the just-shipped
  ticker intake loop all run on subscription auth. Python `BudgetGuard`
  reads the same `total_cost_usd` field from the envelope so global cap
  bookkeeping continues to work across the three loops.

Phase 1 ships first because (a) Rust has the strongest test seam — the new
`LlmBackend` trait is mockable without ever spawning `claude` — so any CLI
envelope surprises get caught there cheaply, (b) the four Rust call sites
fire on every ticker add today via `TickerPrimerService`, so smoke testing
is one `add_ticker` call away. Phase 2 absorbs the lessons: same flag set,
same envelope-parsing logic ported.

## Cross-phase verification

1. **Tracer-bullet (Phase 1 exit):** With `QK_LLM_BACKEND=claude_cli` set
   in `src-tauri/.env` and `ANTHROPIC_API_KEY` empty, restart `pnpm tauri
   dev`, add a fresh symbol via `add_ticker`. Within 30s,
   `news_interpreter` writes a row to `news_cache` with non-zero
   `interpretation` content; `llm_calls` has one new row with
   `kind="news"`, non-zero `output_tokens`, and either a non-zero
   `cost_usd` (parsed from envelope) or a 0 with a corresponding WARN log
   line. Verified by manual smoke + an integration test that drives a fake
   `LlmBackend` returning a canned envelope.
2. **Tracer-bullet (Phase 2 exit):** With the same env var set in the
   agent shell, run `uv run qk-ticker-intake` against a fresh DB; within
   90s a `research_notes` row appears (per the Phase 2 tracer of the
   archived Ticker Intake plan) without any `ANTHROPIC_API_KEY` being
   present in the agent process env.
3. **CI invariant — surveillance-only:** A new test
   (`src-tauri/src/services/llm_service/tests.rs` + an `agent/tests/`
   peer) builds the CLI argv that each backend would invoke and asserts
   the literal substrings `--tools ""`, `--strict-mcp-config`,
   `--mcp-config {}`, `--permission-mode dontAsk` are present. A
   ripgrep-based CI grep additionally asserts that no source file under
   `src-tauri/src/` or `agent/` invokes `claude` outside of the two
   backend modules.
4. **CI invariant — default unchanged:** With `QK_LLM_BACKEND` unset, the
   existing test suite passes byte-for-byte. Phase 1 must not modify any
   existing test in `tests.rs`; it only adds new ones.
5. **CI invariant — no silent fallback:** A unit test asserts that
   constructing `LlmService` with `backend=claude_cli` and a missing
   `claude` binary returns a startup error rather than falling through to
   the API path.
6. **Budget audit:** After Phase 2, an end-to-end test (gated, run
   manually) fires three `news_interpreter` calls and one
   `ticker_intake` agent run with `claude_cli` mode, then asserts (a)
   four new `llm_calls` rows, (b) `BudgetGuard.spent_usd` reflects the
   one agent call, (c) the global cap check (`get_llm_budget_status`
   MCP tool) sees the union of both. CLI envelope cost parsing variance
   is acceptable; token counts must match exactly.

## Open risks

- **CLI JSON envelope shape may shift across versions.** v2.1.126 is what
  the user has today; we'll pin parsing to documented fields (the schema
  passed via `--json-schema` constrains the *result body*, but the
  surrounding envelope's exact shape — `result`, `usage`, `total_cost_usd`,
  `is_error` — is determined by the CLI). Phase 1 documents the observed
  envelope and parses defensively (missing fields → fall through to
  estimation, not panic). If the envelope changes incompatibly in a future
  CLI release, we add a version probe at startup.
- **Subscription-side rate limits.** Pro/Max plan limits aren't published
  per-call; we may see opaque CLI errors under bursty intraday detector
  enrichment. Mitigation: existing 60s subprocess timeout + the fact that
  the heaviest path (per-symbol `news_interpreter`) is already throttled
  by the IBKR rate limiter upstream.
- **Subscription cost is opaque.** The CLI's `total_cost_usd` exists but
  may report 0 for subscription-mode calls (the budget cap is enforced
  CLI-side via `--max-budget-usd` regardless). The ledger's `cost_usd`
  becomes "rough" rather than "exact." Acceptable; the kill-switch still
  works because we always have token counts. Document the caveat in
  `src-tauri/CLAUDE.md` LLM-budget section.
- **Latency.** Subprocess spawn adds ~200-500ms per call. For the four
  Rust call sites that's negligible (each runs once per symbol per event).
  For the agent loops the 60s poll cadence absorbs it.
- **Recursion via the MCP socket.** A `claude -p` instance with default
  config inherits `~/.claude/settings.json`, which lists the `quantum-kapital`
  MCP server. `--strict-mcp-config --mcp-config '{}'` is the unconditional
  prevention. Phase 1 adds a unit test that asserts both flags are passed
  on every invocation. Out-of-scope: protecting against a malicious
  prompt that asks the sub-instance to enable tools — the `--tools ""`
  flag prevents that even if MCP were enabled.
- **`claude` not on PATH.** Some envs (CI containers, fresh installs) lack
  the binary. Phase 1 probes `claude --version` once at startup when the
  `claude_cli` backend is selected and refuses to construct the service
  if absent (per invariant #6). The default `anthropic` mode is unaffected.
- **Migration cost from `ANTHROPIC_API_KEY`.** Existing deployments expect
  the env var. The default stays `anthropic`, so this plan is opt-in and
  zero-touch for anyone not flipping the flag. `src-tauri/.env.example`
  gains a documented `QK_LLM_BACKEND=anthropic` line.
- **Observability drift.** Today the budget MCP tool reports
  `daily_budget_usd` against `cost_today_usd` summed from `llm_calls`. In
  CLI mode, `cost_today_usd` may under-count (envelope reports 0).
  Mitigation: the kill-switch falls back to a token-based estimate via
  `prices::cost_usd` so the cap still trips deterministically. Document
  the observability caveat in the budget tool's docstring.
- **Version pinning.** The CLI surface flagged here (`--json-schema`,
  `--max-budget-usd`) requires v2.1+. Phase 1's startup probe additionally
  asserts a minimum version and surfaces a clear error otherwise.
