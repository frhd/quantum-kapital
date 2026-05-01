# Phase 2 — MCP write tools + research artifacts

> Part of [Quantum Kapital → Autonomous Researcher](master.md). See index for invariants.

**Status:** todo

**Depends on:** Phase 1 (MCP server, read tools)

**Goal:** Add structured-write tools so the agent's output becomes durable in the data spine. New tables for research artifacts. UI surface for browsing.

## Files

- New migration: `src-tauri/migrations/V0X__research_artifacts.sql`
- New: `src-tauri/src/mcp/tools/writes.rs`
- New: `src-tauri/src/services/research_notes/mod.rs` — validation, idempotency, audit
- New: `src-tauri/src/services/mcp_audit/mod.rs` — append-only audit log for all MCP writes
- Touches: `src-tauri/src/services/tracker_service/mod.rs` — `add_ticker` already exists; add `source="agent"` path
- Touches: `src-tauri/src/services/alerts/mod.rs` — `ack_alert(decision, note)`
- New UI: `src/features/research/` — notes list view, morning pack browser
- Touches: `src/features/tracker/` — show research_note links on alerts

## Tools exposed (write)

| Tool | Behavior |
|---|---|
| `add_ticker(symbol, reason, source="agent")` | Idempotent on (symbol). Records `source` provenance in `tracked_tickers`. |
| `archive_ticker(symbol, reason)` | Soft-archive (existing rail from commit `74969a8`). |
| `write_research_note(symbol, body_md, conviction, evidence_refs[], setup_id?)` | Creates row in `research_notes`. `evidence_refs` are typed pointers: `{type:"alert",id:N}`, `{type:"news",cache_id:N}`, `{type:"setup",id:N}`. |
| `write_morning_pack(date, ranked_ideas[])` | Idempotent on date (overwrites). Each idea: `{symbol, thesis_md, conviction, entry_zone, invalidation, evidence_refs}`. |
| `ack_alert(alert_id, decision, note)` | `decision ∈ {acted, passed, researching}`. Note becomes a `research_note` linked to the alert. |

## Tables added

```sql
research_notes(
  id INTEGER PRIMARY KEY,
  symbol TEXT NOT NULL,
  body_md TEXT NOT NULL,
  conviction TEXT,                 -- A | B | C | NULL
  evidence_refs JSON,
  written_by TEXT NOT NULL,        -- "user" | "agent_morning_sweep" | "agent_alert_dive" | etc.
  written_at TIMESTAMP NOT NULL,
  setup_id INTEGER NULL REFERENCES setups(id)
)

mcp_audit(
  id INTEGER PRIMARY KEY,
  tool TEXT NOT NULL,
  input JSON NOT NULL,
  result_summary TEXT,
  caller TEXT,                     -- agent loop name or "interactive"
  called_at TIMESTAMP NOT NULL
)
```

`morning_packs` already exists per recent commits. Extend if schema mismatch with `ranked_ideas` shape above.

## Trust surface

Every write tool:
1. Validates inputs (symbol against IBKR-known list, conviction enum, evidence_ref types).
2. Idempotent on (symbol, date) where applicable.
3. Logs to `mcp_audit` before mutation.
4. Emits an `AppEvent` so UI updates live.

## Reuse

- `tracker_service::add_ticker` (just plumb the new `source` value).
- Soft-archive rail from commit `74969a8`.
- Existing `AppEvent` bus for live UI updates.

## Exit criteria

- From Claude Code: "Research $TSLA and write a note" → research_note appears in React UI within seconds, evidence_refs clickable to source alerts/news.
- Audit log row visible for every write.
- Idempotency tests: same `write_morning_pack(date, ...)` called twice → second overwrites cleanly, no duplicates.
- Surveillance-only CI test: assert no order-mutation tools in MCP registry.

## Gotchas

- **`evidence_refs` schema sprawl.** Lock the type union early or it'll grow unbounded. Start with `alert | news | setup | bar_range`.
- **Conviction taxonomy.** A/B/C is fine; resist adding A+ / A- / B+ until eval data justifies it.
- **UI race conditions.** When agent writes note and emits event, frontend re-fetches — make sure the SWR/query invalidation is wired to the right keys.
