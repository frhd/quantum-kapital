# Phase 23 — Backtest replay mode

## Goal

A `tracker_backtest` command that replays cached daily + intraday bars through the detector registry (and optionally the LLM thesis/ranker layer) over a historical window, returning hit-rate stats per detector. Validates whether what we're surfacing is actually profitable before scaling reliance.

## Depends on

- [ ] Phase 02 — bars cache populated for some history.
- [ ] Phase 06–09 — detectors.
- [ ] Phase 17 / 20 — optional LLM replay (uses `llm_calls` cache so we don't re-bill).

## Out of scope

- Walk-forward optimization / parameter sweeps.
- Slippage modeling beyond a flat `0.05%` round-trip.
- Order-fill realism (we assume entries on close of trigger bar, exits on close of stop / target bar).

## Test plan (write tests FIRST)

`src-tauri/src/services/backtest/tests.rs`.

- [ ] `replays_daily_bars_through_breakout_detector` — synthetic 200-bar fixture with 3 known breakouts; backtest reports 3 signals.
- [ ] `tracks_outcome_2r_target_hit` — fixture where price hits 2R within 20 trading days → outcome `TargetHit`.
- [ ] `tracks_outcome_stop_hit` — price drops to stop → outcome `StopHit`.
- [ ] `tracks_outcome_timeout` — neither target nor stop within window → outcome `Timeout`.
- [ ] `mean_r_achieved_calculation` — across 10 known signals, mean R = correct value.
- [ ] `slippage_applied_round_trip` — entry at 100 with 0.05% slippage → effective entry = 100.05 (long); affects R computation.
- [ ] `llm_replay_uses_cached_calls_when_available` — `llm_calls` row matching `(kind, setup_id)` is reused; mock LLM client recorded zero new calls.
- [ ] `llm_replay_skips_when_no_cache_and_disabled` — `--llm=false` flag → no calls; thesis/ranker fields blank in the report.
- [ ] `report_includes_per_detector_breakdown` — output struct has `per_detector: HashMap<&str, DetectorStats>`.
- [ ] `report_can_be_persisted_to_backtest_results_md` — serialization helper produces a markdown row matching the template in `impl/scratch/backtest-results.md`.

## Implementation tasks

- [ ] Create `src-tauri/src/services/backtest/mod.rs`:
  ```rust
  pub struct Backtest { db, registry, llm_optional }
  pub struct BacktestRequest {
      pub symbols: Vec<String>,                  // empty = use tracker watchlist
      pub start: NaiveDate, pub end: NaiveDate,
      pub use_llm_thesis: bool,
      pub use_llm_ranker: bool,
      pub slippage_bps: u32,                      // default 5
      pub max_holding_days: u32,                  // default 20
  }
  pub struct BacktestReport {
      pub window: (NaiveDate, NaiveDate),
      pub per_detector: HashMap<String, DetectorStats>,
      pub overall: DetectorStats,
      pub generated_at: DateTime<Utc>,
  }
  pub struct DetectorStats {
      pub signals: u32, pub hits: u32, pub stops: u32, pub timeouts: u32,
      pub mean_r: f64, pub median_days_to_target: f64,
  }
  impl Backtest {
      pub async fn run(&self, req: BacktestRequest) -> Result<BacktestReport>;
  }
  ```
- [ ] For each `(symbol, day)` in the window: build a synthetic `MarketContext` from cached bars + cached news + cached fundamentals snapshot (best-effort); run registry; for each `Some(SetupCandidate)`, simulate forward up to `max_holding_days` and classify the outcome.
- [ ] Add Tauri command `tracker_backtest(req: BacktestRequest) -> BacktestReport`.
- [ ] Optional: write the report to `impl/scratch/backtest-results.md` automatically on each run (append a dated entry). Skip if too noisy; user can paste manually.

Frontend (optional, can skip if time-constrained):

- [ ] Create `src/features/tracker/components/BacktestPanel.tsx` — form (symbols / window / toggles), runs the command, renders the report as a small table.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml services::backtest` — green.
- [ ] Manual: run a 90-day backtest over your current watchlist; verify per-detector stats look sensible (breakout hit rate > 30%, parabolic short hit rate higher because of asymmetry, EP varies with news-quality of the window).
- [ ] Append the report to `impl/scratch/backtest-results.md`.
- [ ] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/services/backtest/mod.rs`
- `src-tauri/src/services/backtest/replay.rs` (per-symbol per-day replay loop)
- `src-tauri/src/services/backtest/tests.rs`
- `src/features/tracker/components/BacktestPanel.tsx` (optional)

**Modified:**
- `src-tauri/src/ibkr/commands/tracker.rs` (`tracker_backtest`)
- `src-tauri/src/lib.rs` (register command)

## Scratchpad

- **Read / write** `impl/scratch/backtest-results.md` — append every backtest run with date + window + parameters + report.
- **Cross-reference** `impl/scratch/detector-calibration.md` — when results suggest a parameter is wrong, log a calibration update there.

## Done when

Backtest produces a report with per-detector hit rates, mean R, and median days-to-target on real cached data; LLM replay reuses cached `llm_calls` so re-running is cheap; results write to the scratchpad and inform the next round of detector-calibration.

## Closing note

This phase closes out the original surveillance system. Future expansion paths (not part of this plan): walk-forward optimization, regime detection, watchlist expansion via news triggers, options overlays. Each gets its own design + plan when its turn comes.
