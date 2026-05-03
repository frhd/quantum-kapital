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

