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

### Decay-watcher — v1 (Phase 18 — landed 2026-04-29)

- **Status:** shipped. Source of truth: `src-tauri/src/services/decay_watcher/mod.rs` (`SYSTEM_PROMPT`, `tool_schema()`).
- **Model:** `claude-haiku-4-5`, `max_tokens = 512`. Hot path: every `IntradayScheduler` tick × every `Active` setup on a `SetupActive` ticker (default cadence 5 min during RTH).
- **System prompt summary:** "You watch a single trade setup. Given the original thesis and the most recent bars, decide if it is still valid. Output ONLY through the `emit_decay` tool. Be terse." Spells out the four allowed outcome labels (`still_valid`, `invalidated`, `target_hit`, `thesis_changed`) and requires `reason` to cite a numeric level / bar.
- **Input shape:** two cached system blocks + one user message. System block 0 = persona prompt. System block 1 = per-setup thesis context `{ setup_id, symbol, strategy, direction, trigger_price, stop_price, targets, thesis_md, invalidation_levels[] }` (drawn from `setups.thesis_json`). User message = `{ recent_bars: [{ time, open, high, low, close, volume }] (last 12 Min15 bars), current_quote: f64? }`.
- **Output schema (forced tool-use `emit_decay`):** `{ still_valid: bool, outcome: still_valid|invalidated|target_hit|thesis_changed, reason: string, suggested_action?: string }`. `still_valid` + `outcome` + `reason` required; `suggested_action` is informational only — the system never places orders.
- **Prompt cache TTL:** ephemeral. Both system blocks request `cache_control: { type: "ephemeral" }`. The persona block hits across every setup; the thesis block hits across successive ticks for the same setup (the block embeds `setup_id` so the cache is keyed per-setup). Expected cache-hit rate after the first tick of a setup: ~95% on subsequent ticks within the 5-min Anthropic ephemeral TTL.
- **Freshness grace:** [`FRESHNESS_GRACE = 30 min`](../../src-tauri/src/services/decay_watcher/mod.rs). A setup detected < 30 min ago short-circuits to `DecayDecision::skipped()` without an HTTP call — the first few intraday bars after detection routinely whipsaw and would cause spurious invalidations.
- **Failure handling:** every transient / configuration / parse problem (`BudgetExhausted`, `Auth`, `Upstream`, `Network`, `NoApiKey`, `Malformed`, `UnknownModel`, bars-fetch failure, missing tool call, malformed tool input) collapses to `DecayDecision::skipped()` with a `warn!`. The scheduler treats `Skipped` and `StillValid` identically (no state change), so the budget kill-switch never snowballs into spurious invalidations.
- **Outcome dispatch (intraday scheduler):** `Invalidated | ThesisChanged → state_machine.mark_invalidated(setup_id, reason)`; `TargetHit → state_machine.mark_completed(setup_id)`; `StillValid | Skipped → continue`. `IntradayTickOutcome` gained a `completed_setup_ids: Vec<i64>` field alongside the existing `invalidated_setup_ids` so observers can distinguish the two.
- **Expected cost (per call, Haiku 4.5):** input ~600 tokens (persona + thesis + 12 bars), output ~80 tokens, cache reads after first tick ~500 tokens. Pricing: `(0.0006 × 1) + (0.00008 × 5) + (0.0005 × 0.10) ≈ $0.0011 per cached call` — well under the plan's ~$0.005/call ceiling.
- **Observed:** _to fill in after first real-data run; full end-to-end pass requires `ANTHROPIC_API_KEY` + IBKR TWS + a seeded SetupActive row, all of which are user-driven manual steps_.

### News interpreter — v1 (Phase 19 — landed 2026-04-29)

- **Status:** shipped. Source of truth: `src-tauri/src/services/news_interpreter/mod.rs` (`SYSTEM_PROMPT`, `tool_schema()`).
- **Model:** `claude-haiku-4-5`, `max_tokens = 384`. Triggered after each successful AV news fetch (read-through cache hit short-circuits before reaching the LLM).
- **System prompt summary:** "You read 1–10 news items about one stock. Output ONLY through the `emit_news_verdict` tool. Be terse, neutral, evidence-grounded." Spells out the four output fields and explicitly tells the model to flag `parabolic_risk` even on bullish-tone short-squeeze chatter.
- **Input shape:** one cached system block (persona) + one user message `{ symbol, news: [{ title, summary, source, time_published, overall_sentiment_label, overall_sentiment_score }] (≤ 10 items) }`. The full per-item AV sentiment is included so the model has a numeric prior.
- **Output schema (forced tool-use `emit_news_verdict`):** `{ tone: bullish|bearish|neutral, ep_worthy: bool, parabolic_risk: bool, summary: string }`. All four fields required.
- **Prompt cache TTL:** ephemeral. The single system block is `cache_control: { type: "ephemeral" }`. Cache key is the persona block — every symbol's first interpret call within the 5-min Anthropic ephemeral TTL warms the cache for subsequent symbols.
- **Persistence:** the verdict lands in `news_cache.news_verdict_json` (additive column) on the same row as the AV payload. `INSERT OR REPLACE` semantics on the news fetch path means a fresh payload always clears the prior verdict, so the interpreter re-runs only when the underlying news block changed.
- **Idempotency:** `cached.verdict_json.is_some()` short-circuits the LLM call entirely. A second `interpret(symbol)` within the AV TTL is a no-op.
- **Failure handling:** every transient / config / parse problem (`BudgetExhausted`, `Auth`, `Upstream`, `Network`, `NoApiKey`, `Malformed`, `UnknownModel`, missing tool call) collapses to `Ok(None)` with a `warn!`. The cache row stays verdict-less and the EP detector falls back to AV's per-ticker sentiment score.
- **EP detector consumption:** `EpisodicPivotDetector` prefers the verdict when present — bullish/bearish map to `±VERDICT_SENTIMENT_MAGNITUDE = 0.325` (mid-band of `MIN_SENTIMENT..MAX_SENTIMENT`); neutral falls through to the AV path. The neutral fall-through preserves pre-existing detection capability when the LLM is uncertain.
- **Expected cost (per call, Haiku 4.5):** input ~250 tokens (persona + ≤10 headlines), output ~80 tokens, cache reads after the first call within TTL ~200 tokens. Pricing: `(0.00025 × 1) + (0.00008 × 5) + (0.0002 × 0.10) ≈ $0.0007 per cached call` — well under the plan's ~$0.005/call ceiling.
- **Observed:** _to fill in after first real-data run; manual verification (`tracker_get_news` against a tracked symbol) requires `ANTHROPIC_API_KEY` + `ALPHA_VANTAGE_API_KEY`_.

### Daily ranker — v1 (Phase 20 — landed 2026-04-29)

- **Status:** shipped. Source of truth: `src-tauri/src/services/daily_ranker/mod.rs` (`SYSTEM_PROMPT` via `include_str!("../llm_service/prompts/ranker_v1.md")`, `tool_schema()` via `include_str!("../llm_service/prompts/ranker_tool.json")`).
- **Model:** `claude-sonnet-4-6`, `max_tokens = 1024`. One call per EOD sweep (low frequency); reasoning quality matters because the user reads the rationale verbatim.
- **System prompt summary:** Anchors the model in the user's disciplined-swing risk profile (0.5–1% risk/trade, 5–7 concurrent, 2R/3R targets), spells out ranking principles (prefer A-conviction theses, fresher catalysts, tighter risk; penalize parabolic-risk flags / earnings blackout; pick cleaner invalidation when setups overlap), forces unique contiguous ranks from 1, and demands output ONLY through the `emit_morning_pack` tool. Stored as a `.md` file under `src-tauri/src/services/llm_service/prompts/ranker_v1.md` so version bumps are visible in git history.
- **Input shape (user-message JSON):** `{ top_n: usize, setups: [{ setup_id, symbol, strategy, direction, trigger_price, stop_price, targets, raw_signals, thesis_md, conviction_letter, detected_at }] }`. `conviction_letter` is harvested from `thesis_json.conviction` (Phase 17) when present so the ranker can prefer A/B over C without re-reading the full thesis. Older setups (detected before today's ET-midnight) are excluded by the service before the LLM ever sees them.
- **Output schema (forced tool-use `emit_morning_pack`):** `{ ranked: [{ setup_id (i64), rank (1..10), why_top_pick (string) }] }`. Stored as `src-tauri/src/services/llm_service/prompts/ranker_tool.json` and parsed once at request build time. The service truncates to `top_n` after sorting by rank, so the model can over-deliver without breaking the contract.
- **Prompt cache TTL:** ephemeral. The single system block is `cache_control: { type: "ephemeral" }`. Daily-frequency call so cache hits across days are unlikely; the cache is mostly there for re-runs within the 5-min window if the EOD sweep is retried.
- **Persistence:** the full `MorningPack` JSON lands in `morning_packs(date, payload, generated_at)` keyed by ET trading day; `INSERT ... ON CONFLICT(date) DO UPDATE` so re-runs overwrite (latest wins). The frontend re-fetches via `tracker_get_morning_pack` whenever a `morning-pack-ready` event fires.
- **Failure handling:** every transient / config / parse problem (`BudgetExhausted`, `Auth`, `Upstream`, `Network`, `NoApiKey`, `Malformed`, `UnknownModel`, missing tool call, malformed tool input) collapses to a **naive top-N ranking by detector `conviction_signal` desc** with a `warn!`. The user still gets a pack — the ranker just never blocks the EOD pipeline. Naive entries carry a "Fallback ranking — LLM ranker unavailable" rationale string so the UI is honest about the source.
- **Empty-day path:** zero active setups today → no LLM call, but a `MorningPack { ranked: [] }` is persisted and `MorningPackReady { ranked_count: 0 }` is still emitted so the UI can render an "no setups today" panel rather than going blank.
- **Expected cost (per call, Sonnet 4.6):** input ~1500–2500 tokens (persona + 12 setup summaries with raw_signals), output ~300–500 tokens, cache reads negligible (daily frequency). Pricing: `(0.0015 × 3) + (0.0004 × 15) ≈ $0.011/call` — well within the daily budget for once-per-day execution.
- **Observed:** _to fill in after first real-data run; manual verification via the `tracker_start_scheduler` flow at 16:05 ET requires `ANTHROPIC_API_KEY` + ≥1 detected setup_.

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
