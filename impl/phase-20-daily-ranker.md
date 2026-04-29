# Phase 20 — Daily ranker + MorningPack UI

## Goal

After the EOD detector sweep, send all of today's setups to Sonnet 4.6 in a single ranker call that returns the top 5 with explicit "why this beat the others" reasoning. Surface as a `MorningPack` panel at the top of the Tracker tab.

## Depends on

- [x] Phase 13 — EOD scheduler invokes ranker.
- [x] Phase 16 — LlmService.
- [x] Phase 17 — setups have theses.

## Out of scope

- Per-setup execution suggestions beyond what theses already include.
- Ranker invocation outside EOD (no on-demand re-rank for now).

## Test plan (write tests FIRST)

`src-tauri/src/services/daily_ranker/tests.rs`.

- [x] `builds_request_with_all_todays_setups` — given 12 active setups detected today, request payload includes all 12 (id, symbol, strategy, direction, conviction_signal, thesis_md, conviction_letter, key levels). Older setups excluded.
- [x] `forces_emit_morning_pack_tool_use` — `tool_choice = ForceTool("emit_morning_pack")`.
- [x] `parses_ranked_top_n` — mock returns `{ranked: [{setup_id, rank, why_top_pick}, ...]}`; service returns a `MorningPack { date, ranked }` with at most `top_n` entries.
- [x] `persists_morning_pack_to_db` — store the ranker response in a new table `morning_packs(date PRIMARY KEY, payload JSON, generated_at INTEGER)`.
- [x] `dedup_per_date` — second call same date overwrites (latest wins); `morning-pack-ready` event re-emitted.
- [x] `respects_budget_kill_switch` — `BudgetExhausted` → emits a `MorningPack` with the naive top-5 (ordered by `conviction_signal` desc) and logs a warn; user still gets a list.
- [x] `empty_setups_today_skips_call` — zero setups → no LLM call; emits empty `MorningPackReady`.

## Implementation tasks

- [x] Add `morning_packs` table to `schema.sql`. Even though Phase 01 baked all tables upfront, this is one we deferred — log it in `schema-decisions.md`.
- [x] Create `src-tauri/src/services/daily_ranker/mod.rs`:
  ```rust
  pub struct DailyRanker { llm, db, emitter }
  pub struct MorningPack {
      pub date: NaiveDate,
      pub ranked: Vec<RankedSetup>,
      pub generated_at: DateTime<Utc>,
  }
  pub struct RankedSetup { pub setup_id: i64, pub rank: u8, pub why_top_pick: String }
  impl DailyRanker {
      pub async fn rank_today(&self, date: NaiveDate, top_n: usize) -> Result<MorningPack>;
  }
  ```
- [x] System prompt (`prompts/ranker_v1.md`): "You are ranking today's swing-trade candidates. The user has a disciplined risk profile (0.5–1% per trade, 5–7 concurrent). Pick the top N with the cleanest setup, freshest catalyst, and best risk/reward — explain *why each beat the others*. Output ONLY through the `emit_morning_pack` tool."
- [x] Tool schema (`prompts/ranker_tool.json`):
  ```json
  { "name": "emit_morning_pack",
    "input_schema": {"type":"object","properties":{
      "ranked":{"type":"array","items":{"type":"object","properties":{
        "setup_id":{"type":"integer"},"rank":{"type":"integer","minimum":1,"maximum":10},
        "why_top_pick":{"type":"string"}},"required":["setup_id","rank","why_top_pick"]}}},
    "required":["ranked"]}}
  ```
- [x] Hook into `EodScheduler` (Phase 13) — after `runner.run_all` and `expire_ttls`, call `daily_ranker.rank_today(today_et, 5)`.
- [x] Update `AppEvent::MorningPackReady` (Phase 15) to carry the full `MorningPack` payload (or its ID — frontend fetches via a new command).
- [x] Add Tauri command `tracker_get_morning_pack(date: Option<NaiveDate>) -> Option<MorningPack>` (defaults to most recent).

Frontend:

- [x] Create `src/features/tracker/components/MorningPack.tsx`:
  - Sticky card at top of Tracker tab.
  - Shows date + 5 ranked rows.
  - Each row: symbol, strategy chip, conviction badge, "why top pick" expandable, "Open analysis" button.
  - Collapsible.
- [x] Hook into `useTrackerEvents` to refresh when `morning-pack-ready` fires.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml services::daily_ranker` — green.
- [x] Manual at 16:05 ET (or with the mocked clock) with at least 5 setups detected today: MorningPack panel appears with explanations.
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/daily_ranker/mod.rs`
- `src-tauri/src/services/daily_ranker/tests.rs`
- `src-tauri/src/services/llm_service/prompts/ranker_v1.md`
- `src-tauri/src/services/llm_service/prompts/ranker_tool.json`
- `src/features/tracker/components/MorningPack.tsx`

**Modified:**
- `src-tauri/src/storage/schema.sql` (`morning_packs` table)
- `src-tauri/src/services/eod_scheduler.rs` (call ranker)
- `src-tauri/src/events/emitter.rs` (`MorningPackReady` payload extension)
- `src-tauri/src/ibkr/commands/tracker.rs` (`tracker_get_morning_pack`)
- `src-tauri/src/lib.rs` (register command)
- `src/features/tracker/components/TrackerTab.tsx` (mount MorningPack)

## Scratchpad

- **Read / write** `impl/scratch/llm-prompts.md` Ranker section.
- **Read / write** `impl/scratch/schema-decisions.md` for the new table.
- **Read** `impl/scratch/backtest-results.md` later to compare LLM-ranked top-5 vs naive top-5.

## Done when

EOD sweep produces a MorningPack with explanations; UI surfaces it at the top of the Tracker tab; budget kill-switch yields a naive ranking instead of failing; users can re-fetch yesterday's pack via the command.
