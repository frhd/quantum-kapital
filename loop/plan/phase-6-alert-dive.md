# Phase 6 — Per-alert deep-dive agent

> Part of [Quantum Kapital → Autonomous Researcher](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-02)

**Depends on:** Phase 1 (MCP read), Phase 2 (MCP write — `write_research_note`)

**Goal:** Event-driven enrichment. When the tracker fires an alert, an agent loop attaches deep research within 1-2 minutes — before you see the alert.

## Files

- New: `agent/alert_dive.py` — polling loop on `get_alerts(since=watermark, unenriched_only=true)`
- Touches: `src-tauri/src/mcp/tools/reads.rs` — extend `get_alerts` with `since` and `unenriched_only` filters
- New MCP tool: `mark_alert_enriched(alert_id, research_note_id)` — idempotency marker
- Touches: `src-tauri/src/services/alerts/mod.rs` — alert payload schema gains `research_note_id` field
- New migration: `ALTER TABLE alerts ADD COLUMN enriched_at TIMESTAMP NULL, research_note_id INTEGER NULL REFERENCES research_notes(id)`
- New: `agent/cron/alert_dive.service` (systemd) — long-running poller, restart-on-failure
- Touches: UI alert detail view — show "Enriching..." → "Deep dive ready" state with note link

## Loop logic

1. Every 30s: `get_alerts(since=last_seen, unenriched_only=true, limit=10)`.
2. For each alert (parallelizable across alerts, throttle to N concurrent):
   - Gather: `get_news(symbol, since=7d)`, `get_sentiment(symbol, since=7d)`, `get_setups(symbol, since=90d)`, `get_fundamentals(symbol)`, `get_quote(symbol)`, `get_bars(symbol, "5m", 78)`
   - Synthesize: `write_research_note(symbol, body_md, conviction, evidence_refs=[{type:"alert",id:N}, ...], setup_id=...)`
   - `mark_alert_enriched(alert_id, research_note_id)`
3. Update `last_seen` watermark.
4. Sleep 30s; repeat.

## Budget guardrail

- Per-alert USD cap (default `$0.05`).
- If global budget < 10% remaining, skip enrichment silently with a `skipped_low_budget` audit row.
- Emit `AlertDiveSkipped` event so UI can show "deep dive skipped (budget)".

## Why polling, not webhook/IPC

- Polling is simplest — no new transport, no event bridge from Rust → external Python.
- 30s latency is acceptable for the use case (you're not staring at the alert log).
- Watermark pattern is robust to crashes — never miss an alert.
- Phase 9 daemon could replace this with a Unix-socket event push if latency matters.

## Reuse

- All MCP tools from Phases 1+2.
- `LlmService` budget enforcement.
- Existing alert event bus (read-only).

## Exit criteria

- Tracker fires alert → research note attached within 1-2 minutes.
- UI alert view shows enrichment state transitions.
- `mark_alert_enriched` is idempotent (re-call with same alert_id is a no-op).
- Crash-recovery: kill the loop mid-flight; on restart, resumes from watermark with no duplicate enrichments.

## Gotchas

- **Polling watermark race.** Use `enriched_at IS NULL` as the source of truth, not the watermark — watermark is just a perf hint. Otherwise crash-restart can miss alerts that fired after watermark was advanced but before enrichment completed.
- **Burst alerts.** Tracker can fire 5-10 alerts simultaneously (e.g., market open). Throttle concurrent enrichments (default 2-3) to avoid budget blowout.
- **Symbol-correlated alerts.** Two alerts for the same symbol within minutes — second can reference first's note instead of redoing the work. Add a "recent note for symbol" lookup before full synthesis.
