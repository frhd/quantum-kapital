# Phase 2 — Python `ClaudeCliLlmClient` + agent loop wiring

> Part of [LLM backend split](master.md). See index for invariants.

**Status:** done (commit 3f43b30, 2026-05-03)

**Depends on:** 1

**Goal:** Mirror Phase 1 in `agent/llm.py`. Add `ClaudeCliLlmClient`
implementing the existing `LlmClient` Protocol, wire `agent/config.py`
to read the same `QK_LLM_BACKEND` env var, and make all three loops
(`morning_sweep`, `alert_dive`, `ticker_intake`) construct the right
client based on the flag. `BudgetGuard` continues to track per-loop
spend; cost is parsed from the same CLI envelope shape Phase 1 pinned
down.

## Files

- Touches: `agent/llm.py` — add `ClaudeCliLlmClient` class next to
  `AnthropicLlmClient`. Both implement the existing `LlmClient`
  Protocol (`async def call`). Add module-level
  `make_llm_client(backend: str) -> LlmClient` factory. Add
  `BackendError` exception class for non-zero subprocess exits /
  unparseable envelopes. If size pushes past 200 LOC, split into
  `agent/llm_anthropic.py` + `agent/llm_cli.py` with `agent/llm.py`
  re-exporting.
- Touches: `agent/config.py` — add `llm_backend: str` field, populated
  from `QK_LLM_BACKEND` env (default `anthropic`). Surface it on the
  shared `Settings` object the loops construct from.
- Touches: `agent/config.toml` — document the field and accepted
  values.
- Touches: `agent/morning_sweep.py`, `agent/alert_dive.py`,
  `agent/ticker_intake.py` — replace the direct `AnthropicLlmClient(...)`
  construction with `make_llm_client(settings.llm_backend)`. Three
  identical edits.
- Touches: `agent/budget_guard.py` — no behavior change; verify the
  existing `record_call(input_tokens, output_tokens, cost_usd)`
  signature accepts the parsed envelope cost. If the CLI envelope
  reports zero cost under subscription auth, fall back to the
  per-token estimate from a small Python-side pricing table that
  mirrors `src-tauri/src/services/llm_service/prices.rs` exactly.
  Keep both pricing tables in lockstep (note in `agent/README.md`).
- New: `agent/prices.py` — minimal pricing table mirroring the Rust
  one. Used as the per-token fallback when CLI cost is absent.
- Touches: `agent/tests/test_llm_cli.py` (new) — argv-construction
  test, envelope-parsing test, no-`ANTHROPIC_*`-env-leak test, and a
  factory-selection test (`make_llm_client("anthropic")` returns
  `AnthropicLlmClient`; `"claude_cli"` returns `ClaudeCliLlmClient`).
- Touches: `agent/tests/test_morning_sweep.py`,
  `agent/tests/test_alert_dive.py`, `agent/tests/test_ticker_intake.py`
  — assert each loop pulls the backend from settings (do not hardcode
  `AnthropicLlmClient`). Use the existing `FakeLlmClient` test
  fixture for the actual call surface.
- Touches: `agent/README.md` — add a short "LLM backend selection"
  section: env var name, both values, why subscription mode skips
  per-call USD cost reporting, the lockstep with Rust pricing.
- Touches: `agent/cron/*.service` (`alert_dive.service`,
  `ticker_intake.service`, plus `morning_sweep` if it has a unit) —
  inherit `QK_LLM_BACKEND` from the user environment so a single
  shell-level flip toggles all three loops.

## Reuse (no new business logic this phase)

- The `LlmClient` Protocol (`agent/llm.py:34`) is unchanged. Both
  clients return the same `LlmResponse` dataclass.
- `BudgetGuard` (`agent/budget_guard.py`) is unchanged; the new
  client simply emits the cost field via the same path.
- The CLI argv shape, env hygiene rules, and version-probe semantics
  are copied from Phase 1's `cli_backend.rs` — no design churn this
  phase. If Phase 1 logged a `QUESTIONS.md` deferral about the
  envelope, this phase honors that decision.
- `tools` / `tool_choice` mapping is the same: single forced-tool's
  `input_schema` becomes `--json-schema`; `Auto` and multi-tool raise.

## Decisions to make in this phase

- **Subprocess library.** Decision: `asyncio.create_subprocess_exec`
  (no shell, args list). Avoids quoting bugs; matches Rust's
  `Command::arg` discipline.
- **Pricing table duplication.** The Rust `prices.rs` is
  authoritative; Python mirrors it. Decision: a unit test in
  `agent/tests/test_prices.py` parses the Rust file's `match` arms
  with a small regex and asserts every model + rate matches the
  Python table — keeps both in lockstep without a build-time
  generator. If parsing turns brittle, downgrade to a CI grep + a
  comment-anchored hand-sync ritual.
- **Cost-zero envelope handling.** If CLI reports `total_cost_usd =
  0` under subscription auth, decision: estimate via the Python
  pricing table from envelope tokens and tag the `BudgetGuard`
  ledger entry with `cost_source="estimate"` (new optional column
  on `LlmCall` if the existing one allows; otherwise just a debug
  log). Don't let a zero envelope cost make `BudgetGuard` think
  inference is free.
- **Version probe placement.** Decision: probe once per loop
  startup (not per call). The probe lives on
  `ClaudeCliLlmClient.__init__` and raises `BackendError` on
  failure, which the loop catches and exits with a clear message
  (mirrors how each loop handles a missing API key today).

## Exit criteria

- `QK_LLM_BACKEND` unset or `anthropic` — `uv run pytest` from
  `agent/` passes; the three loops continue to construct
  `AnthropicLlmClient` exactly as today. No existing test is
  modified — only new ones added or assertions extended.
- `QK_LLM_BACKEND=claude_cli` — each loop starts cleanly without
  `ANTHROPIC_API_KEY` set; `make_llm_client` returns the CLI
  variant; envelope parsing produces a `LlmResponse` with non-zero
  `input_tokens` / `output_tokens`.
- Argv unit test mirrors Phase 1's: every always-on flag is present
  in the assembled argv; no `ANTHROPIC_*` leaks into the child
  env; `--bare` is not passed.
- Pricing-table sync test: the Rust + Python tables agree on every
  model.
- Tracer-bullet end-to-end (manual, recorded in PR): with
  `QK_LLM_BACKEND=claude_cli` exported in the agent shell and
  `ANTHROPIC_API_KEY` unset, run `uv run qk-ticker-intake`. Add a
  fresh symbol via the MCP `add_ticker` tool. Within 90s a
  `research_notes` row exists with `written_by="agent.ticker_intake"`
  (per the writer-discipline note in
  `loop/plan/done/ticker-intake-enrichment/QUESTIONS.md`, the
  persisted column will read `"interactive"` until the Rust caller
  fix lands; assertion is on the Python writer constant via the
  test fake).
- `BudgetGuard.spent_usd` reflects the ticker-intake call after
  the tracer; if the envelope reported 0, the per-token fallback
  produced a non-zero estimate. Either way, the loop's per-loop
  cap remains enforceable.
- One startup INFO log line per loop: `llm: backend=claude_cli
  version=2.1.126`.
- `agent/README.md` documents the new env var + lockstep pricing
  + cost-source caveat.
- File-size caps: `agent/llm.py` stays under the 200-line soft cap
  the existing file holds to (currently 107). If size pressure
  forces a split, do it cleanly per the Files section above.

## Gotchas

- **`written_by` discipline still client-side only.** The
  `loop/plan/done/ticker-intake-enrichment/QUESTIONS.md` Phase 2
  entry documents that the in-process MCP server overwrites
  `written_by` with `"interactive"`. This phase does not fix that;
  the tracer-bullet asserts the call shape via the test fake (same
  pattern Phase 2 of the archived plan used).
- **Loops cache the client.** `morning_sweep`, `alert_dive`, and
  `ticker_intake` each build the client once at startup. Restart
  the process to pick up an env-var change; document this in
  `agent/README.md` so a flip + reload pattern is obvious.
- **`asyncio` + subprocess on macOS vs Linux.** Both supported;
  Python's `asyncio.create_subprocess_exec` is the canonical
  cross-platform path. Tests must use `unittest.mock.patch` on the
  module-level subprocess factory so they don't actually spawn
  `claude`. The existing `FakeLlmClient` pattern is the test seam.
- **Stripping parent env.** Tests must assert that
  `os.environ["ANTHROPIC_API_KEY"]` (when set) does NOT appear in
  the subprocess env when the CLI client is used. The CLI's auth
  precedence prefers the env var over OAuth/keychain, so leaking
  it would silently disable subscription-mode behavior.
- **Working directory.** Same as Phase 1: spawn in a fresh
  `tempfile.TemporaryDirectory()` to prevent project CLAUDE.md
  pickup. Clean up on call completion.
- **Per-call timeout.** 60s, same as Phase 1. Use
  `asyncio.wait_for` around the subprocess `communicate()`; on
  timeout, `proc.kill()` then `await proc.wait()` to reap.
- **Stdin encoding.** Pass UTF-8; reject non-string content with
  a typed error rather than relying on Python's implicit
  encoding behavior.
- **Per-loop budget interaction with global cap.** The three
  loops still share the daily global cap via the
  `get_llm_budget_status` MCP tool. With CLI mode under-reporting
  cost (envelope zero), the global cap may be enforced more by
  the per-loop USD ceiling than by the daily ledger in
  practice. Document; don't fix here.
- **systemd unit env-var loading.** `agent/cron/*.service` uses
  `EnvironmentFile=` or `Environment=` directives. Decide which
  one matches the existing pattern and add `QK_LLM_BACKEND` the
  same way; do not invent a new mechanism.
- **Two separate Python pricing tables risk.** The agent already
  has `agent/data_summary.py` and `agent/synthesizer.py` as
  pricing-adjacent code. Verify there isn't already a stub
  pricing table elsewhere before adding `agent/prices.py`; reuse
  if so.
