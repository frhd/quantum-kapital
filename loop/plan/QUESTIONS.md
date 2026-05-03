# Questions / cross-phase issues — LLM backend split

Append-only log under `## Phase N (YYYY-MM-DD)` headings. Use for issues raised during execution that the phase intentionally did NOT fix: pre-existing flakes, scope-cut deferrals, decisions punted to a later phase.

Each entry should name the file/test/symbol so the next maintainer pass can find it.

---

<!-- entries land below as phases run -->

## Phase 1 (2026-05-03)

### `--mcp-config '{}'` is rejected by claude v2.1.126

Master plan + phase-1 doc spec the literal `--mcp-config '{}'`, but the
binary rejects bare `{}` with `Invalid MCP configuration: mcpServers:
Does not adhere to MCP server configuration schema`. The minimal payload
the CLI accepts is `{"mcpServers":{}}`, which is what `ClaudeCliBackend`
ships. Same intent (empty MCP server set, no inheritance from
`~/.claude/settings.json`), correct spelling. Argv unit test asserts the
exact `{"mcpServers":{}}` literal so a future CLI version that tightens
the schema fails the test rather than silently inheriting MCP entries.

### Observed CLI envelope shape (claude v2.1.126)

Canonical fields used by `ClaudeCliBackend::parse_envelope`:

- `is_error: bool` — backend treats `true` as a fatal `LlmError::Backend`.
- `result: string` — text body when no `--json-schema` is in use; empty
  string when the structured-output path runs.
- `structured_output: object` — the JSON the schema constrained, present
  only when `--json-schema` was passed.
- `usage.{input_tokens, output_tokens, cache_read_input_tokens,
  cache_creation_input_tokens}` — accurate token counts (used by the
  ledger when `total_cost_usd` is missing or zero).
- `total_cost_usd: number` — best-effort; observed 0.0078 on a 6.4k-input
  haiku call, so the field is populated under subscription auth too.
  Stored as `LlmResponse.cost_usd_override` when > 0.

Recorded in `cli_backend.rs` doc comment so future maintainers can spot
incompatible envelope drift.

### End-to-end tracer-bullet deferred to manual smoke

Phase 1 exit criteria call for a manual smoke run with
`QK_LLM_BACKEND=claude_cli` set in `src-tauri/.env`,
`ANTHROPIC_API_KEY` empty, then `add_ticker AMD` driving
`news_interpreter` to a populated `news_cache` row. The unit-test
surface (29 tests including 17 new) covers argv assembly, every
surveillance flag, envelope parsing for both structured and text
paths, `total_cost_usd` override threading, `is_error=true` →
`LlmError::Backend`, version probe success/failure, and
multi-tool / `tool_choice=Auto` / multi-turn rejection. The full
end-to-end requires a live IBKR/TWS connection, so it's left for the
user to verify on their dev box per the master plan ("manual, recorded
in PR"). No code change pending from this smoke unless the envelope
shape diverges from v2.1.126.

## Phase 2 (2026-05-03)

### Pricing table reuses `agent/budget_guard.py::_PRICES_USD_PER_MTOK`

The phase doc proposed a new `agent/prices.py` module mirroring
`src-tauri/.../prices.rs`. Following the master plan's "two separate
Python pricing tables risk" gotcha, we reused the existing table in
`agent/budget_guard.py` rather than introducing a new module. The
sync test (`agent/tests/test_prices.py`) parses the Rust file's
`match` arms and asserts every `(input, output)` rate matches the
Python dict. Asymmetry: Python additionally lists `claude-opus-4-7`
(opus is the over-count fallback for unknown models); the test pins
that asymmetric set so a maintainer who adds opus to the Rust side
remembers to extend it there too.

### `LlmResponse.cost_usd` optional field

`ClaudeCliBackend` (Rust) carries `total_cost_usd` via
`LlmResponse.cost_usd_override`; the Python mirror uses the simpler
name `cost_usd: float | None = None`. The Anthropic SDK does not
surface a per-call USD figure, so the API path leaves it `None`. Any
non-positive envelope value is normalized to `None` in
`ClaudeCliLlmClient.parse_envelope` so subscription-mode `0` figures
don't make `BudgetGuard` think inference is free.

### End-to-end tracer-bullet deferred to manual smoke

The Phase 2 exit criteria's tracer-bullet (`uv run qk-ticker-intake`
under `QK_LLM_BACKEND=claude_cli` driving a `research_notes` write
within 90s of `add_ticker`) requires a live Tauri app + IBKR/TWS
connection. The unit-test surface (19 new across `test_llm_cli.py` +
`test_prices.py` + 3 new `test_budget_guard.py` cases) covers argv
assembly, envelope parsing both structured and text, env hygiene
(`ANTHROPIC_*` strip), version probe success/failure, factory
selection, and pricing-table sync. End-to-end is left to the user
per the master plan's "manual, recorded in PR" convention.

### Unrelated working-tree change to `src-tauri/src/ibkr/client/market_data.rs`

A modification to `market_data.rs` was present in the working tree
during Phase 2 execution but was untouched by the phase work
(snapshot dispatch / streaming-drain / `SnapshotMode` is unrelated
to the LLM backend split). It was excluded from Phase 2's two
commits and remains as a working-tree change for whoever owns it.

