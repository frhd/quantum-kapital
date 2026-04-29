# LLM prompts scratchpad

Versioned prompt history for thesis / decay-watcher / news-interpreter / daily-ranker. Used to track what worked, what didn't, and to enable prompt-replay in backtest mode.

Use this when:
- Authoring a new prompt (Phases 17, 18, 19, 20).
- Iterating on a prompt after observing bad output (any phase that ships LLM output).
- Configuring prompt caching (record cache TTL chosen and observed cache-hit rate).

---

## Conventions

- Each prompt has a stable `kind` (`thesis`, `decay`, `news`, `ranker`) and a monotonic `version` integer.
- `version` is bumped on **any** functional change: system prompt edit, schema change, model change, cache TTL change.
- Persisted in code under `src-tauri/src/services/llm_service/prompts/` (created in Phase 17). Each prompt = one `.md` or `.rs` const + a JSON tool-input schema.
- This scratchpad records *why* a version exists. The code is source of truth for *what*.

---

## Models in use

| Job | Model | Reason | First used |
|---|---|---|---|
| Thesis | `claude-sonnet-4-6` | Reasoning quality matters; called once per setup detection (low frequency) | Phase 17 |
| Decay-watcher | `claude-haiku-4-5` | High frequency (every 5 min × N in-play); cheap, structured | Phase 18 |
| News interpreter | `claude-haiku-4-5` | Per-news-item classification; cheap, narrow task | Phase 19 |
| Daily ranker | `claude-sonnet-4-6` | Once daily; reasoning over multiple setups | Phase 20 |

If switching models, bump the prompt version and log here.

---

## Prompt versions

### Thesis — v1 (Phase 17 — landed 2026-04-29)

- **Status:** shipped. Source of truth: `src-tauri/src/services/thesis_generator/mod.rs` (`SYSTEM_PROMPT`, `tool_schema()`).
- **Model:** `claude-sonnet-4-6`, `max_tokens = 1024`.
- **System prompt summary:** "sober swing trader's analyst", cite numeric `raw_signals`, never narrate a chart you can't see, output ONLY through the `emit_thesis` tool. Style guidance: concise, evidence-first, name the strategy + reason from the structured signals, list concrete invalidation levels with prices, risk-flag unusual factors (low float, dilution, earnings blackout).
- **Input shape (user-message JSON):** `{ setup: { id, symbol, strategy, direction, trigger_price, stop_price, targets, raw_signals, detected_at }, bars_summary: [{ time, close, volume, daily_pct }] (≤ 20 most-recent daily bars), recent_news: [{ title, summary, source, time_published, overall_sentiment_label }] (≤ 5 items) }`. Fundamentals + live quote intentionally omitted in v1 since `TrackerRunner::context_for` does not fetch them today; will roll in once a fundamentals fetcher lives on the runner.
- **Output schema (forced tool-use `emit_thesis`):** `{ thesis_md (string, 80–250 words), conviction (enum A|B|C), invalidation_levels[{label, price, reason}], risk_notes (string) }`. All four fields required.
- **Prompt cache TTL:** ephemeral. The single system block is marked `cache_control: { type: "ephemeral" }` so successive thesis calls in a sweep amortize the prompt cost.
- **Persistence:** the markdown body lands in `setups.thesis`; the full structured object is serialized to `setups.thesis_json` (Phase 17 schema additions).
- **Failure handling:** transient or config LLM errors (BudgetExhausted, Auth, Upstream, Network, NoApiKey, Malformed, UnknownModel, Storage, Serde) all collapse to `Ok(None)` with a `warn!` — the row stays thesis-less and the runner falls back to emitting `SetupDetected { thesis: None }`.
- **Idempotency:** `setup.thesis.is_some()` short-circuits the LLM call entirely, so the EOD/intraday schedulers can re-evaluate a setup without burning tokens.
- **Observed:** _to fill in after first real-data run; `tracker_llm_smoke_test` + Phase 17 manual verification still pending the user's `ANTHROPIC_API_KEY` walk-through_.

### Decay-watcher — v1 (Phase 18)

_to fill_

### News interpreter — v1 (Phase 19)

_to fill_

### Daily ranker — v1 (Phase 20)

_to fill_

---

## Observed token / cost log

Append after each prompt evaluation:

```
### YYYY-MM-DD — <kind> v<N>
- Model: ...
- Input tokens (avg): ...
- Output tokens (avg): ...
- Cache read tokens (avg): ...
- Cache hit rate: ...
- Avg cost per call: $...
- Quality observations: ...
```
