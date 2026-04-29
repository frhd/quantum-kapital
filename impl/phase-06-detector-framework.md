# Phase 06 — Strategy detector framework

## Goal

Land the abstract types — `StrategyDetector` trait, `MarketContext`, `SetupCandidate`, `DetectorRegistry` — so concrete detectors in Phases 07–09 are pure logic with zero plumbing.

## Depends on

- [ ] Phase 02 — `HistoricalBar` is fetchable.
- [ ] Phase 03 — `NewsItem` exists.
- [ ] Phase 04 — `TrackedTicker`, `StrategyTag` exist.

## Out of scope

- Concrete detector implementations (Phases 07–09).
- Anything that runs detectors (Phase 10 / 13 / 14).

## Test plan (write tests FIRST)

`src-tauri/src/strategies/tests.rs`. The framework itself is mostly types, but the registry has behavior worth testing:

- [ ] `registry_evaluate_all_runs_each_detector_once` — register 3 mock detectors; `evaluate_all(&ctx)` returns 3 results in deterministic order.
- [ ] `registry_evaluate_filters_by_tag` — `evaluate_for_tags(&ctx, &[StrategyTag::Breakout])` only invokes the breakout detector.
- [ ] `registry_collects_errors_without_short_circuiting` — one mock returns `Err(...)`; the others still run; aggregate is `Vec<Result<...>>`.
- [ ] `setup_candidate_targets_at_2r_3r_match_risk_profile` — given trigger=100, stop=98, helper `targets_for_risk_profile(direction, trigger, stop)` returns `[2R=104, 3R=106]` for long; mirror for short.
- [ ] `targets_helper_handles_zero_risk_distance` — degenerate case (trigger == stop) returns `Err(...)` rather than `inf`.

## Implementation tasks

- [ ] Create `src-tauri/src/strategies/mod.rs`:
  ```rust
  pub use trait_def::{StrategyDetector, DetectorError};
  pub use context::MarketContext;
  pub use candidate::{SetupCandidate, Direction, TargetLevel};
  pub use registry::DetectorRegistry;
  ```
- [ ] `src-tauri/src/strategies/trait_def.rs`:
  ```rust
  #[async_trait]
  pub trait StrategyDetector: Send + Sync {
      fn name(&self) -> &'static str;
      fn tag(&self) -> StrategyTag;
      fn timeframe(&self) -> BarSize;
      fn min_lookback_days(&self) -> u32;
      async fn evaluate(&self, ctx: &MarketContext)
          -> Result<Option<SetupCandidate>, DetectorError>;
  }
  ```
- [ ] `src-tauri/src/strategies/context.rs`:
  ```rust
  pub struct MarketContext<'a> {
      pub symbol: &'a str,
      pub daily_bars: &'a [HistoricalBar],
      pub intraday_bars: Option<&'a [HistoricalBar]>,
      pub fundamentals: Option<&'a FundamentalData>,
      pub recent_news: &'a [NewsItem],
      pub current_quote: Option<&'a MarketDataSnapshot>,
      pub now: DateTime<Utc>,
  }
  ```
- [ ] `src-tauri/src/strategies/candidate.rs`:
  ```rust
  pub enum Direction { Long, Short }
  pub struct TargetLevel { pub label: String, pub price: f64 }
  pub struct SetupCandidate {
      pub strategy: &'static str,
      pub tag: StrategyTag,
      pub direction: Direction,
      pub conviction_signal: f64, // 0..1, pre-LLM
      pub trigger_price: f64,
      pub stop_price: f64,
      pub targets: Vec<TargetLevel>,
      pub raw_signals: serde_json::Value,
      pub timeframe: BarSize,
      pub detected_at: DateTime<Utc>,
  }
  pub fn targets_for_risk_profile(
      direction: Direction, trigger: f64, stop: f64,
  ) -> Result<Vec<TargetLevel>, &'static str> { /* 2R, 3R */ }
  ```
- [ ] `src-tauri/src/strategies/registry.rs`:
  ```rust
  pub struct DetectorRegistry { detectors: Vec<Arc<dyn StrategyDetector>> }
  impl DetectorRegistry {
      pub fn new() -> Self { ... }
      pub fn register(&mut self, d: Arc<dyn StrategyDetector>) { ... }
      pub async fn evaluate_all(&self, ctx: &MarketContext) -> Vec<DetectorOutcome>;
      pub async fn evaluate_for_tags(&self, ctx: &MarketContext, tags: &[StrategyTag]) -> Vec<DetectorOutcome>;
  }
  pub struct DetectorOutcome {
      pub detector: &'static str,
      pub result: Result<Option<SetupCandidate>, DetectorError>,
  }
  ```
- [ ] Add `mod strategies;` to `src-tauri/src/lib.rs`.
- [ ] Optionally expose `DetectorRegistry` on `IbkrState` (skip if unused this phase; Phase 10 will wire it).

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml strategies::` — green.
- [ ] `cargo clippy ... -D warnings` — no warnings on the new module (the trait must be `Send + Sync` for use across threads — confirm this compiles when stored in `Arc<dyn StrategyDetector>`).
- [ ] `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/strategies/mod.rs`
- `src-tauri/src/strategies/trait_def.rs`
- `src-tauri/src/strategies/context.rs`
- `src-tauri/src/strategies/candidate.rs`
- `src-tauri/src/strategies/registry.rs`
- `src-tauri/src/strategies/tests.rs`

**Modified:**
- `src-tauri/src/lib.rs` (`mod strategies;`)

## Scratchpad

None.

## Done when

All four type files compile, registry tests pass, the codebase still builds, no concrete detector exists yet (deliberate — they ride in Phase 07+).
