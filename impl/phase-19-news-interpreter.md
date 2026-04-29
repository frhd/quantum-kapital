# Phase 19 — News interpreter prompt

## Goal

When `news_cache` is refreshed for a tracked symbol, ask Haiku 4.5 to classify the most recent news items: tone (bullish/bearish/neutral), `ep_worthy` (could this drive an episodic-pivot setup), `parabolic_risk` (does this catalyst feel exhaustion-y), terse summary. Persist the verdict alongside the cache.

## Depends on

- [ ] Phase 03 — news fetch + cache.
- [ ] Phase 16 — LlmService.

## Out of scope

- Real-time news streaming (we poll AV).
- Multi-language.
- Summary across many tickers (Phase 20 ranker uses these per-ticker verdicts).

## Test plan (write tests FIRST)

`src-tauri/src/services/news_interpreter/tests.rs`.

- [ ] `interprets_bullish_earnings_beat` — fixture news set with positive earnings → mock returns `{tone: "bullish", ep_worthy: true, parabolic_risk: false, summary: ...}`; service stores it.
- [ ] `interprets_bearish_guidance_cut` — mock returns `{tone: "bearish", ep_worthy: true, ...}`.
- [ ] `interprets_neutral_routine_filing` — neutral 10-K filing → `{tone: "neutral", ep_worthy: false, ...}`.
- [ ] `flags_parabolic_risk_on_short_squeeze_chatter` — fixture with "short squeeze" headlines → `parabolic_risk: true`.
- [ ] `persists_to_news_cache_payload` — verdict stored as a sibling JSON column or as part of the cached payload (decide and log in scratchpad).
- [ ] `does_not_call_llm_when_no_new_news` — second call within TTL with no new items → no LLM call.
- [ ] `respects_budget_kill_switch` — `BudgetExhausted` → returns `Ok(NewsVerdict::skip())`; service logs and proceeds.
- [ ] `caches_news_block_per_symbol` — cache_control on the news block.
- [ ] `verdict_drives_ep_detector_in_phase_08_revisit` — Phase 08's EP detector reads the verdict (when present) instead of raw sentiment scores. (This phase adds the data; the EP detector update is also in this phase or in a follow-on tweak — see Implementation tasks.)

## Implementation tasks

- [ ] Decide storage. **Recommendation:** add a `news_verdict_json TEXT` column to `news_cache` (additive). Log in `schema-decisions.md`.
- [ ] Create `src-tauri/src/services/news_interpreter/mod.rs`:
  ```rust
  pub struct NewsInterpreter { llm, db }
  pub struct NewsVerdict {
      pub tone: NewsTone,                 // Bullish | Bearish | Neutral
      pub ep_worthy: bool,
      pub parabolic_risk: bool,
      pub summary: String,
  }
  impl NewsInterpreter {
      pub async fn interpret(&self, symbol: &str) -> Result<NewsVerdict>;
  }
  ```
- [ ] System prompt (`prompts/news_v1.md`): "You read 1–10 news items about one stock. Output ONLY through the `emit_news_verdict` tool. Be terse, neutral, evidence-grounded."
- [ ] Tool schema (`prompts/news_tool.json`):
  ```json
  { "name": "emit_news_verdict",
    "input_schema": { "type": "object",
      "properties": {
        "tone": {"type": "string", "enum": ["bullish","bearish","neutral"]},
        "ep_worthy": {"type": "boolean"},
        "parabolic_risk": {"type": "boolean"},
        "summary": {"type": "string"}
      }, "required": ["tone","ep_worthy","parabolic_risk","summary"] } }
  ```
- [ ] Hook into the news refresh path in `financial_data_service.rs` — after a fresh fetch from AV, kick off `news_interpreter.interpret(symbol)` (best-effort; log on error).
- [ ] Update `EpisodicPivotDetector` (Phase 08) to prefer `NewsVerdict` when present (use `verdict.tone` to disambiguate sentiment polarity, fall back to AV's `overall_sentiment_score` otherwise). Tests added: detector picks up verdict when present.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml services::news_interpreter` — green.
- [ ] Manual: `tracker_get_news('NVDA', 24)` → triggers refresh → `news_cache.news_verdict_json` populated.
- [ ] Manual: trigger an EP setup against a ticker with a verdict and confirm the EP detector uses the verdict.
- [ ] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/news_interpreter/mod.rs`
- `src-tauri/src/services/news_interpreter/tests.rs`
- `src-tauri/src/services/llm_service/prompts/news_v1.md`
- `src-tauri/src/services/llm_service/prompts/news_tool.json`

**Modified:**
- `src-tauri/src/storage/schema.sql` (additive `news_verdict_json`)
- `src-tauri/src/services/financial_data_service.rs` (post-fetch hook)
- `src-tauri/src/strategies/episodic_pivot/detector.rs` (consume verdict)
- `src-tauri/src/strategies/episodic_pivot/tests.rs` (new fixture: prefers verdict)
- `src-tauri/src/ibkr/state.rs`

## Scratchpad

- **Read / write** `impl/scratch/llm-prompts.md` News section.
- **Write** `impl/scratch/schema-decisions.md` for the additive column.

## Done when

News refresh produces verdicts; EP detector consumes them; budget kill-switch handled gracefully; AV-only fallback still works when LLM is disabled.
