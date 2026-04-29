# Phase 22 — Configurable detector parameters

## Goal

Move the hardcoded constants from each detector into `AppConfig.detectors` so they can be tuned without recompiling. Settings persist in `~/.config/quantum-kapital/settings.json` like everything else.

## Depends on

- [x] Phases 07, 08, 09 — detectors exist with hardcoded constants.

## Out of scope

- A UI for editing the constants. For now they're edited via `update_settings` command and a JSON editor; a real settings UI is a follow-on.
- Per-symbol overrides (e.g., "use a tighter ATR multiplier for low-float names"). Future.

## Test plan (write tests FIRST)

`src-tauri/src/strategies/config_tests.rs`.

- [x] `default_detector_config_matches_phase_07_through_09_constants` — `DetectorsConfig::default()` produces the same values currently hardcoded (regression guard).
- [x] `breakout_detector_uses_configured_volume_multiple` — set `breakout.volume_multiple = 2.0`; case where vol mult was 1.7 (passed default) now fails to fire.
- [x] `episodic_pivot_detector_uses_configured_min_gap_pct` — set `episodic_pivot.min_gap_pct = 0.06`; case at 5% gap no longer fires.
- [x] `parabolic_short_detector_uses_configured_consec_days` — set `parabolic_short.min_consec_days = 4`; 3-day case now misses.
- [x] `serializing_settings_round_trips` — `AppConfig` with detector tweaks → save → load → values preserved.
- [x] `missing_detector_section_falls_back_to_defaults` — older `settings.json` without the `detectors` block loads cleanly.

## Implementation tasks

- [x] Add `pub detectors: DetectorsConfig` to `AppConfig` in `src-tauri/src/config/settings.rs`.
- [x] Define:
  ```rust
  pub struct DetectorsConfig {
      pub breakout: BreakoutCfg,
      pub episodic_pivot: EpisodicPivotCfg,
      pub parabolic_short: ParabolicShortCfg,
  }
  pub struct BreakoutCfg {
      pub lookback_days: u32,           // default 20
      pub volume_multiple: f64,          // default 1.5
      pub rsi_ceiling: f64,              // default 80.0
      pub atr_period: u32,               // default 14
      pub swing_low_period: u32,         // default 10
  }
  pub struct EpisodicPivotCfg {
      pub min_gap_pct: f64,              // default 0.04
      pub min_sentiment_abs: f64,        // default 0.15
      pub min_volume_ratio: f64,         // default 1.0
  }
  pub struct ParabolicShortCfg {
      pub min_consec_days: u32,          // default 3
      pub min_per_day_move: f64,         // default 0.05
      pub min_cumulative_move: f64,      // default 0.40
      pub min_atr_distance: f64,         // default 2.0
      pub min_rsi: f64,                  // default 80.0
  }
  ```
  All `serde::{Serialize, Deserialize}` with `#[serde(default)]` on each field so missing sections degrade gracefully.
- [x] Update each detector's constructor to accept its config struct (`BreakoutDetector::with_config(cfg: BreakoutCfg)`); `Default::default()` produces the original behavior.
- [x] Build the `DetectorRegistry` from the loaded `AppConfig.detectors` in `lib.rs::run` (where the registry is constructed today; `IbkrState::new` does not own the registry).
- [x] Update existing detector tests to call `Detector::default()` (was `Detector` unit-struct); explicit-cfg fixtures live in the new `config_tests.rs`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml strategies::config_tests` — green (6/6).
- [x] `cargo test --manifest-path src-tauri/Cargo.toml strategies::` — all detector tests still green after parameterization (58/58).
- [ ] Manual: edit `~/.config/quantum-kapital/settings.json` to set `detectors.breakout.volume_multiple = 3.0`, restart, verify fewer hits. (skipped — no live IBKR session).
- [x] `cargo clippy ... -D warnings`, `cargo fmt --check` — both clean.

## Files

**Created:**
- `src-tauri/src/strategies/config.rs`
- `src-tauri/src/strategies/config_tests.rs`

**Modified:**
- `src-tauri/src/config/settings.rs` (`detectors` block)
- `src-tauri/src/strategies/breakout/detector.rs` (accept cfg)
- `src-tauri/src/strategies/episodic_pivot/detector.rs` (accept cfg)
- `src-tauri/src/strategies/parabolic_short/detector.rs` (accept cfg)
- `src-tauri/src/strategies/mod.rs` (registry construction takes cfg)
- `src-tauri/src/ibkr/state.rs` (pass cfg into registry)

## Scratchpad

- **Read** `impl/scratch/detector-calibration.md` — the threshold rationale lives there. Defaults in code mirror the table.
- **Write** to `detector-calibration.md` whenever a default is changed: rationale, observed lift in hit rate.

## Done when

Detector behavior is fully driven by `AppConfig.detectors`; defaults match Phase 07–09 baselines exactly; settings round-trip through disk; defaults applied on missing sections.
