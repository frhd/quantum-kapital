# Phase 07 — Breakout detector

## Goal

A working `BreakoutDetector` that fires long-only on daily-timeframe new-high closes with volume confirmation and a sensible swing-low stop. Fully unit-tested with synthetic OHLCV.

## Depends on

- [ ] Phase 06 — `StrategyDetector` trait + `MarketContext` + `SetupCandidate` exist.

## Out of scope

- Bear-side breakouts (covered by Parabolic Short detector, Phase 09).
- Sub-daily breakouts.
- Adaptive thresholds — initial values are constants in code; Phase 22 makes them configurable.

## Test plan (write tests FIRST)

`src-tauri/src/strategies/breakout/tests.rs` with table-driven cases. Each case is a `BarsFixture` (Vec<HistoricalBar>) + expected outcome.

- [ ] `fires_on_new_20d_high_with_volume_confirmation` — synthetic series rises to a new 20d-high close on day T with volume = 2× the 20d avg → `Some(SetupCandidate)` with `direction = Long`, `trigger_price = close[T]`, `stop_price = swing_low_10`, `targets = [2R, 3R]`.
- [ ] `does_not_fire_without_volume` — same price action but volume = 0.8× avg → `None`.
- [ ] `does_not_fire_when_not_a_new_high` — price near but not at the 20d high → `None`.
- [ ] `does_not_fire_when_rsi_above_80` — strong uptrend with RSI(14) at 85 → `None` (overextended).
- [ ] `requires_min_lookback` — fewer than 20 bars in `daily_bars` → `Err(DetectorError::InsufficientHistory)`.
- [ ] `stop_uses_min_of_swing_low_and_atr_distance` — case where swing_low_10 = trigger - 0.4×ATR (tighter) → stop = swing_low; case where swing_low_10 = trigger - 1.5×ATR → stop = trigger - 1×ATR.
- [ ] `targets_are_2r_and_3r_above_trigger_for_long` — given trigger=100, stop=98 → targets `[label='2R', price=104]`, `[label='3R', price=106]`.
- [ ] `raw_signals_includes_volume_multiple_atr_swing_low` — JSON has keys `lookback_high`, `volume_multiple`, `atr_14`, `swing_low_10`, `rsi_14`.
- [ ] `conviction_signal_scales_with_volume_multiple` — vol mult 1.5 → ~0.5; vol mult 3.0 → ~0.85; clamped to [0,1].
- [ ] `degenerate_zero_atr_does_not_panic` — flat-line bars (all close == open == high == low) → `None` rather than divide-by-zero.

## Implementation tasks

- [ ] Create `src-tauri/src/strategies/breakout/mod.rs` exposing `BreakoutDetector`.
- [ ] Create `src-tauri/src/strategies/breakout/detector.rs` implementing `StrategyDetector`:
  - `name() = "breakout"`, `tag() = StrategyTag::Breakout`, `timeframe() = BarSize::Day1`, `min_lookback_days() = 30`.
  - `evaluate`:
    1. Validate lookback.
    2. Compute `lookback_high = max(close[-20..-1])` (exclusive of today).
    3. Compute `vol_avg = mean(volume[-20..-1])`, `vol_mult = volume[-1] / vol_avg`.
    4. Compute `atr_14`, `swing_low_10`, `rsi_14` via helpers.
    5. Trigger if `close[-1] >= lookback_high && vol_mult >= 1.5 && rsi_14 < 80`.
    6. `stop = max(swing_low_10, close[-1] - atr_14)` (the *higher* / tighter stop for longs).
    7. Build candidate with `targets_for_risk_profile(Long, trigger, stop)`.
- [ ] Helper module `src-tauri/src/strategies/indicators.rs` with `atr(bars, period)`, `rsi(bars, period)`, `swing_low(bars, period)`, `swing_high(bars, period)`. Independent unit tests against TA-Lib reference values for known fixtures.
- [ ] Register the detector in `DetectorRegistry::default()`.

## Verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml strategies::breakout` — all green.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml strategies::indicators` — green.
- [ ] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/strategies/breakout/mod.rs`
- `src-tauri/src/strategies/breakout/detector.rs`
- `src-tauri/src/strategies/breakout/tests.rs`
- `src-tauri/src/strategies/indicators.rs` (with tests inline)

**Modified:**
- `src-tauri/src/strategies/mod.rs` (re-export + register)

## Scratchpad

- **Read** `impl/scratch/detector-calibration.md` for breakout threshold rationale.
- **Write** chosen constants and observations to `impl/scratch/detector-calibration.md` Breakout section. After Phase 10/13 produces real hits, append observation entries.

## Done when

`BreakoutDetector` fires correctly on every test case, indicators match a published reference (e.g., StockCharts SCTR sample values to within rounding), no panics on degenerate inputs.
