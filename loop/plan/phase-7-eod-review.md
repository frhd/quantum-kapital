# Phase 7 — EOD review + journal integration

> Part of [Quantum Kapital → Autonomous Researcher](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-02)

**Depends on:** Phase 1, 2 (MCP), Phase 5 (morning packs to score)

**Goal:** After-close agent loop scores yesterday's predictions vs today's outcomes; appends a calibration section to today's journal entry.

## Files

- New: `agent/eod_review.py`
- New service: `src-tauri/src/services/outcome_extractor/mod.rs` — given a morning-pack prediction (entry_zone, invalidation), looks up actual price action and computes outcome class
- New MCP tools (extend `mcp/tools/reads.rs` and `writes.rs`):
  - `get_morning_pack(date)` — full pack with predictions
  - `get_outcomes(since)` — joined with bars, returns realized outcome per prediction
  - `append_journal_entry(date, section, body_md)` — append-only by section
- Touches: `.claude/skills/daily-journal/` — extend skill to surface agent-written sections
- Touches: journal markdown template at `journal/YYYY-MM-DD.md` — reserve "EOD Review (Agent)" section
- New cron entry: 17:00 ET weekdays

## Outcome class taxonomy

For each morning-pack prediction:
- `hit_entry` — price entered the entry_zone
- `hit_target` — price reached defined target after entry
- `hit_invalidation` — price hit the invalidation level
- `drifted` — neither entry nor invalidation reached
- `no_movement` — price stayed within ±0.5% of entry zone all day

Conviction-weighted scoring: A-conviction `hit_target` worth more than C-conviction `hit_target`; A-conviction `hit_invalidation` is the most damaging miscall.

## Loop logic

1. `get_morning_pack(yesterday)` → list of predictions.
2. For each: `get_outcomes(symbol, since=yesterday_open)` → outcome class + evidence (bars + actual highs/lows).
3. LLM commentary: which calls played out, which didn't, why; calibration notes; surprising outcomes.
4. `append_journal_entry(today, "EOD Review (Agent)", body_md)`.
5. Also write structured rows to `outcomes` table (Phase 8 will use these).

## Reuse

- Existing `daily-journal` skill at `.claude/skills/daily-journal/`.
- `bars_cache` for outcome extraction.
- Trading calendar for "yesterday" semantics (skip weekends/holidays correctly).

## Exit criteria

- Daily journal at `journal/YYYY-MM-DD.md` includes an "EOD Review (Agent)" section scoring yesterday's morning pack.
- `outcomes` table populated with one row per prediction per day.
- Closed loop visible: morning pack → end-of-day outcome → journal commentary in <24h.

## Gotchas

- **Outcome timing.** "Hit target" needs a window — if target hits a week later, do we credit the morning-pack call? Default: same-day for `hit_entry`/`hit_invalidation`, 5 trading days for `hit_target`. Make configurable.
- **Survivorship.** Only scored predictions get evaluated. If agent wrote "no high-conviction picks today, market choppy," that's also a data point — record it as `outcome_class=skipped` with realized regret (best opportunity missed that day).
- **Journal merge conflicts.** If you also write to today's journal manually, agent's append must not clobber. Append-only by section name with structured markers (`<!-- agent:eod_review:start --> ... <!-- agent:eod_review:end -->`).
