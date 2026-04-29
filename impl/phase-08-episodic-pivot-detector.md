# Phase 08 — Episodic Pivot detector

## Goal

A working `EpisodicPivotDetector` that fires bidirectionally on news-driven gaps where sentiment polarity aligns with gap direction and first-30-min volume confirms institutional flow.

## Depends on

- [x] Phase 06 — detector framework.
- [x] Phase 03 — `NewsItem` + sentiment fields available in `MarketContext`.
- [x] Phase 02 — intraday bars fetchable (needed for first-30-min volume).

## Out of scope

- 8-K / earnings parsing — we rely on AV's aggregated `overall_sentiment_score` and `ticker_sentiment`.
- Premarket gap detection — gaps are computed against prior-day close vs RTH-open.

## Test plan (write tests FIRST)

`src-tauri/src/strategies/episodic_pivot/tests.rs`. Fixtures combine daily bars + intraday bars + news items.

- [x] `fires_long_on_gap_up_with_bullish_news` — gap = +6%, news with `overall_sentiment_score = 0.4` published in last 18h, first-30-min volume ≥ prior day total → `Some` with `direction = Long`.
- [x] `fires_short_on_gap_up_with_bearish_news` — gap = +6%, news sentiment = -0.3, first-30-min volume ≥ prior day total → `Some` with `direction = Short` (suspect rally; common EP-short setup).
- [x] `fires_short_on_gap_down_with_bearish_news` — gap = -5%, sentiment = -0.4 → `direction = Short`.
- [x] `does_not_fire_without_news` — gap = +6% but `recent_news` is empty → `None`.
- [x] `does_not_fire_with_neutral_sentiment` — sentiment score within ±0.15 → `None`.
- [x] `does_not_fire_below_min_gap` — gap = 2% (below 4% threshold) → `None`.
- [x] `does_not_fire_without_volume_confirmation` — first-30-min volume < prior day total → `None`.
- [x] `requires_intraday_bars` — `intraday_bars = None` and gap qualifies → `Err(DetectorError::IntradayBarsRequired)`.
- [x] `stop_for_long_is_pre_gap_close` — long EP, stop = previous day's close.
- [x] `stop_for_short_is_gap_day_high` — short EP, stop = today's high so far.
- [x] `raw_signals_includes_gap_pct_sentiment_volume_ratio` — JSON has those keys.
- [x] `most_relevant_news_item_drives_sentiment` — when multiple news items exist, the one with highest `relevance_score` for the symbol determines polarity.

## Implementation tasks

- [x] Create `src-tauri/src/strategies/episodic_pivot/mod.rs` exposing `EpisodicPivotDetector`.
- [x] Create `src-tauri/src/strategies/episodic_pivot/detector.rs`:
  - `name() = "episodic_pivot"`, `tag() = StrategyTag::EpisodicPivot`, `timeframe() = BarSize::Min15`, `min_lookback_days() = 5`.
  - `evaluate`:
    1. Require `intraday_bars`.
    2. Compute gap = `(open_today - close_yesterday) / close_yesterday`.
    3. Filter to `|gap| >= 0.04`.
    4. Pick best `NewsItem` by `relevance_score` for the symbol; require `|sentiment_score| >= 0.15`.
    5. Determine direction: `if (gap > 0 && sentiment > 0) || (gap < 0 && sentiment < 0)` → continuation (Long for gap up, Short for gap down). Else if `gap > 0 && sentiment < 0` → fade short. Else `None`.
    6. Volume confirmation: sum of first-30-min intraday volume vs `prior_day_volume` (from daily bars). Require `>=` parity.
    7. Stops: long → `close_yesterday`; short → `max(high) of intraday_bars so far`.
    8. Targets: 2R, 3R via helper.
- [x] Compute `conviction_signal` from a blend: `0.4 * normalize(|gap|, 0.04..0.10) + 0.4 * normalize(|sentiment|, 0.15..0.5) + 0.2 * normalize(volume_ratio, 1..3)` — clamp to [0, 1].
- [x] Register in `DetectorRegistry::default()`.

## Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml strategies::episodic_pivot` — all green.
- [x] `cargo clippy ...`, `cargo fmt --check`.

## Files

**Created:**
- `src-tauri/src/strategies/episodic_pivot/mod.rs`
- `src-tauri/src/strategies/episodic_pivot/detector.rs`
- `src-tauri/src/strategies/episodic_pivot/tests.rs`

**Modified:**
- `src-tauri/src/strategies/mod.rs` (re-export + register)

## Scratchpad

- **Read** `impl/scratch/detector-calibration.md` for EP thresholds (gap 4%, sentiment 0.15, volume ratio 1.0).
- **Write** chosen constants and observations after Phase 10 / 13 produce real fires.

## Done when

EP detector fires on long-continuation, short-continuation, and short-fade scenarios; refuses to fire without aligned sentiment or without volume confirmation; uses correct stops per direction; all tests green.
