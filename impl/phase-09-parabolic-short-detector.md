# Phase 09 — Parabolic Short detector

## Goal

A working `ParabolicShortDetector` that identifies blow-off-top names ready to fade and triggers on the first red 15-min bar after the parabolic sequence.

## Depends on

- [x] Phase 06 — detector framework.
- [x] Phase 02 — intraday bars fetchable.
- [x] Phase 07 — indicators module (`atr`, `rsi`).

## Out of scope

- Long-side mirror detector (not part of this watchlist).
- Hedging suggestions / pair trades.

## Test plan (write tests FIRST)

`src-tauri/src/strategies/parabolic_short/tests.rs`.

- [x] `fires_on_classic_blow_off_with_first_red_15m` — daily bars: 4 consecutive up days at +6%, +8%, +5%, +12% with cumulative move > 40%; price ≥ 2× 20d ATR above 20d MA; RSI(14) = 84; intraday: first red 15-min bar after the open → `Some` with `direction = Short`.
- [x] `does_not_fire_without_first_red_bar` — same daily setup but no intraday red bar yet → `None` (waiting for trigger).
- [x] `does_not_fire_below_consec_minimum` — only 2 consecutive up days → `None`.
- [x] `does_not_fire_below_per_day_minimum` — 4 days but one is +3% (below 5% floor) → `None`.
- [x] `does_not_fire_below_cumulative_move` — 5 consecutive up days but total only +18% (below 40%) → `None`.
- [x] `does_not_fire_when_not_extended_above_ma` — price within 1×ATR of 20d MA despite consecutive ups → `None`.
- [x] `does_not_fire_with_low_rsi` — sequence qualifies but RSI(14) = 65 → `None`.
- [x] `stop_is_session_high` — short stop equals max(high) of today's intraday bars.
- [x] `raw_signals_includes_consec_days_cumulative_move_atr_distance_rsi` — JSON has those keys.
- [x] `targets_are_2r_3r_below_trigger_for_short` — given trigger=100, stop=104 → `2R = 92`, `3R = 88`.
- [x] `requires_intraday_bars` — `intraday_bars = None` → `Err(DetectorError::IntradayBarsRequired)`.

## Implementation tasks

- [x] Create `src-tauri/src/strategies/parabolic_short/mod.rs` exposing `ParabolicShortDetector`.
- [x] Create `src-tauri/src/strategies/parabolic_short/detector.rs`:
  - `name() = "parabolic_short"`, `tag() = StrategyTag::ParabolicShort`, `timeframe() = BarSize::Min15`, `min_lookback_days() = 25`.
  - `evaluate`:
    1. Require ≥ 25 daily bars + intraday bars.
    2. Find longest tail of consecutive up days from the most recent bar; compute `consec_days`, `cumulative_move`, `min_per_day_move`.
    3. Filter: `consec_days >= 3 && min_per_day_move >= 0.05 && cumulative_move >= 0.40`.
    4. Compute `ma_20`, `atr_20`, `distance_above_ma = (close - ma_20) / atr_20`. Require `>= 2.0`.
    5. Compute `rsi_14` on daily bars; require `>= 80`.
    6. Trigger condition: scan today's intraday bars for the first bar where `close < open` (a red bar). If none yet → `None`.
    7. `trigger_price = close` of that red bar (or current close if past the red bar).
    8. `stop_price = max(high)` of today's intraday bars so far.
    9. Targets via `targets_for_risk_profile(Short, trigger, stop)`.
- [x] `conviction_signal = 0.3 * normalize(consec_days, 3..6) + 0.3 * normalize(cumulative_move, 0.4..0.8) + 0.2 * normalize(distance_above_ma, 2..4) + 0.2 * normalize(rsi_14, 80..95)`.
- [x] Register in `DetectorRegistry::default()`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml strategies::parabolic_short` — all green.
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/strategies/parabolic_short/mod.rs`
- `src-tauri/src/strategies/parabolic_short/detector.rs`
- `src-tauri/src/strategies/parabolic_short/tests.rs`

**Modified:**
- `src-tauri/src/strategies/mod.rs` (re-export + register)

## Scratchpad

- **Read / write** `impl/scratch/detector-calibration.md` parabolic-short section.

## Done when

Detector fires on canonical blow-off setups, refuses to fire without all five conditions (consec, per-day, cumulative, ATR distance, RSI), needs an actual intraday red bar to trigger, all tests green.
