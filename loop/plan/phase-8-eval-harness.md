# Phase 8 — Eval harness

> Part of [Quantum Kapital → Autonomous Researcher](master.md). See index for invariants.

**Status:** done (commit 29d8dc6, 2026-05-02)

**Depends on:** Phase 5 (data starts flowing). Dashboard meaningful at ~30 trading days.

**Goal:** Calibration tracking, cost vs signal-quality attribution, drift detection. Without this, "actionable advice" is theater.

## Files

- New migrations:
  - `predictions(id, source, symbol, conviction, entry_zone, invalidation, target, predicted_at, morning_pack_id)`
  - `outcomes(prediction_id, outcome_class, realized_at, evidence_json, days_to_resolution)`
- New service: `src-tauri/src/services/eval_harness/mod.rs`
- Touches: `services/research_notes/` and morning pack writes — auto-create `predictions` rows from morning pack ideas
- Touches: outcome extractor (Phase 7) — auto-create `outcomes` rows
- New MCP tools:
  - `get_calibration_stats(window)` — A/B/C win rates over N days
  - `get_prediction_history(symbol, since)` — agent self-introspection
  - `get_cost_attribution(window)` — LLM spend bucketed by loop, vs realized prediction quality
- New UI: `src/features/eval/` — calibration dashboard
  - 30-day rolling win rate per conviction tier
  - Cost vs signal scatter (LLM $ spent per high-conviction call vs hit_target rate)
  - Drift chart (calibration over time — getting better or worse?)

## Backtest hooks (stretch)

- Replay mode: feed historical bars + cached news/sentiment to agent loops in dry-run.
- Useful for: tuning prompts, evaluating new detectors, regression testing prompt changes.
- Implementation: a `--replay-from DATE` flag on `agent/morning_sweep.py` that mocks `get_quote` / time-shifts `get_bars` to historical date.

## Reuse

- Morning pack predictions (Phase 5) — auto-extract on write.
- Outcome extractor (Phase 7) — already running daily.
- `llm_calls` ledger for cost attribution.

## Exit criteria

- Eval dashboard shows: 30-day calibration table per conviction tier, total LLM spend vs # high-conviction calls vs realized outcome class.
- You can answer "is the agent net positive?" with data, not vibes.
- Drift detection: alert if A-conviction win rate drops >X% week-over-week.

## What "good" looks like

After 30 trading days:
- A-conviction `hit_target` rate clearly higher than B and C.
- Cost per high-conviction call < some defined ceiling (e.g., $1).
- No catastrophic mass-miscall events (>3 A-conviction `hit_invalidation` in a single day).

If these don't hold: prompt iteration, model change, or scope reduction. The eval harness exists to catch this fast.

## Gotchas

- **Outcome class is reductive.** A `drifted` outcome could be a slow winner that took 2 weeks. Add a `revisit_at` follow-up so eval can re-score later. Don't let early drifts permanently bias calibration.
- **Conviction inflation.** Agent will be tempted to mark everything A-conviction if A wins more attention. Force conviction distribution check ("at least 30% C calls") in the synthesis prompt.
- **Cost attribution gotcha.** The MCP tool calls themselves don't cost LLM tokens; only the agent's `messages.create` calls do. `llm_calls` ledger captures the right thing already, but make sure agent loops are tagging calls with `loop_name` for attribution.
- **First 30 days are noisy.** Resist tuning based on small samples. Set a "no major changes for first 30 days" rule for the morning_sweep prompt.
