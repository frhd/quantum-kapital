# Detector calibration scratchpad

Per-detector threshold choices, why they were picked, and observations from running them on real bars.

Use this when:
- Picking initial parameters for a detector (Phases 07, 08, 09).
- Tuning thresholds after observing real-world hit rates (Phases 10, 23).
- Adding a new detector — copy the template below.

---

## Breakout detector

| Parameter | Initial value | Source / rationale | Notes after live observation |
|---|---|---|---|
| Lookback for high | 20 trading days, exclusive of today | Standard "one month" breakout convention | Implemented Phase 07; awaits live hits |
| Volume multiple | ≥ 1.5× 20-day avg (same exclusive window as the high) | Common breakout volume confirmation | _to fill_ |
| RSI(14) ceiling | strict `< 80` | Avoid already-extended names | _to fill_ |
| ATR period for stop | 14, Wilder smoothing seeded with SMA of the first 14 TRs | Standard | _to fill_ |
| Stop distance | `max(swing_low_10, trigger − 1×ATR)` (the *higher* / tighter of the two for longs) | Tight but not micro-stop | _to fill_ |
| Targets | 2R, 3R | Risk profile (disciplined swing) | _to fill_ |
| Direction | Long-only (Phase 07) | Bear-side breakouts handled by parabolic-short detector instead | _to fill_ |
| `min_lookback_days` | 30 | Buffer above the 21 strictly required (20-day high + today, plus 14-period ATR/RSI warm-up) | _to fill_ |
| Conviction signal | logistic `1 / (1 + exp(−1.2·(vol_mult − 1.5)))`, clamped `[0,1]` | Midpoint at the trigger threshold (1.5×) → conviction = 0.5 there; ≈0.86 at 3.0×; smooth, monotonic | Tweak `k` if backtest shows top-decile clustering |
| Degenerate-input guard | Returns `None` when `trigger_price ≤ stop_price` (e.g., flat OHLC where ATR = 0 and swing_low = close) | Prevents zero-risk candidates and divide-by-zero in `targets_for_risk_profile` | _to fill_ |

## Episodic Pivot detector

| Parameter | Initial value | Source / rationale | Notes |
|---|---|---|---|
| Min gap % | 4% | Bonde EP literature: meaningful gap floor | _to fill_ |
| First-30min volume vs prior day | ≥ same as full prior day | Confirms institutional flow | _to fill_ |
| Sentiment alignment | required | News must agree with gap direction | _to fill_ |
| Sentiment score floor | \|0.15\| | AV NEWS_SENTIMENT scale; tweak after live data | _to fill_ |
| Stop (long) | pre-gap close | EP invalidates if gap fills | _to fill_ |
| Stop (short) | gap-day high | Mirror image | _to fill_ |

## Parabolic Short detector

| Parameter | Initial value | Source / rationale | Notes |
|---|---|---|---|
| Consecutive up days | ≥ 3, strict-greater (`close[i] > close[i-1]`); equal closes break the streak | "Parabolic" minimum sequence; strict-greater keeps flat-baseline fixtures clean | Implemented Phase 09 |
| Per-day move floor | min `(close[i] − close[i-1]) / close[i-1] ≥ 5%` across the streak | Filters out single-day grinders embedded in a larger move | _to fill_ |
| Cumulative move | `(today.close − prior_close) / prior_close ≥ 40%`, where `prior_close` = bar just before the streak | "Blow-off" threshold; measured from streak entry, not a fixed 5-day window | _to fill_ |
| MA period | 20 (simple mean of last 20 closes) | Standard 1-month MA | _to fill_ |
| ATR period | 20, Wilder smoothing seeded with SMA of first 20 TRs | Match MA window so distance is in self-consistent units | _to fill_ |
| Distance above MA | `(close − ma_20) / atr_20 ≥ 2.0` | Stretched-rubber-band confirmation; rejects names that are up 40%+ but inside their own volatility cone | _to fill_ |
| RSI(14) floor | `≥ 80` (Wilder smoothing) | Overbought confirmation; flat-input convention `RSI = 50` cannot accidentally trigger | _to fill_ |
| Trigger | close of first 15-min intraday bar where `close < open` | Avoid catching falling knife pre-rollover; demands actual rejection candle | _to fill_ |
| Stop | `max(high)` of today's intraday bars so far (session high) | Tight short stop; invalidates if price reclaims the session top | _to fill_ |
| Targets | 2R / 3R below trigger via `targets_for_risk_profile(Short, …)` | Risk profile (disciplined swing) | _to fill_ |
| Direction | Short-only | Bear-side counterpart to long breakouts | _to fill_ |
| `min_lookback_days` | 25 (recommended fetch window); internal gate is 21 (ATR(20) needs 21 bars) | Mirror episodic_pivot's split between scheduler-hint and strict-required | _to fill_ |
| Conviction signal | `0.3·norm(consec, 3..6) + 0.3·norm(cumul, 0.40..0.80) + 0.2·norm(atr_dist, 2..4) + 0.2·norm(rsi, 80..95)`, clamped `[0,1]` | Equal weight on persistence + extension; lighter weight on overextension to avoid double-counting RSI/ATR distance | Tweak weights if backtest skews toward shallow setups |
| Degenerate-input guards | `Ok(None)` when `prior_close ≤ 0`, `atr_20 == 0`, or `stop_price ≤ trigger_price` | Prevents divide-by-zero and zero-risk candidates | Mirrors breakout detector pattern |

---

## Phase 10 dedup decision

The runner short-circuits a re-run when `recent_duplicate(symbol, strategy, direction, 24h)` returns `Some(id)`. We chose **skip the insert and leave the existing row's `detected_at` untouched** rather than touching it forward. Rationale:

- `detected_at` is the moment the signal first fired; mutating it on every subsequent run obscures the original signal time.
- The `last_checked_at` column on `tracked_tickers` already records "we looked at this ticker again," so the audit trail of repeated passes is preserved without trampling the setup row.
- 24h window is a conservative single-day guard. Phase 12 (status state machine) will revisit when transitions like `Active → Invalidated` need to gate re-emission on lifecycle, not just elapsed time.

## Observation log

Append dated observations as detectors run on live data:

```
### YYYY-MM-DD — <detector name>
- Symbol(s): ...
- Hit rate (true positive / total fired): X / Y
- False positive themes: ...
- Suggested adjustment: ...
```
