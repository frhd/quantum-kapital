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

### Thesis — v1 (Phase 17)

- **Status:** _to write in Phase 17_
- **System prompt summary:** describes role (sober swing trader's analyst), rules (cite numeric `raw_signals` only, never narrate a chart you can't see), output schema (tool-use enforced).
- **Input shape:** `SetupCandidate` + recent bars summary (last 20 daily) + fundamentals (P/E, market cap, latest year revenue/EPS) + recent news headlines + current quote.
- **Output schema:** `{thesis_md, conviction: A|B|C, invalidation_levels[{label, price, reason}], risk_notes}`.
- **Prompt cache TTL:** ephemeral (5 min for intraday batches).
- **Observed:** _to fill in after first run_.

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
