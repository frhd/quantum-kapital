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
| Consecutive up days | ≥ 3 | "Parabolic" minimum sequence | _to fill_ |
| Per-day move floor | ≥ 5% | Filters out grinders | _to fill_ |
| Cumulative 5-day move | ≥ 40% | "Blow-off" threshold | _to fill_ |
| Distance above 20MA | ≥ 2× 20-day ATR | Stretched-rubber-band confirmation | _to fill_ |
| RSI(14) floor | ≥ 80 | Overbought confirmation | _to fill_ |
| Trigger | first red 15-min bar after sequence | Avoid catching falling knife pre-rollover | _to fill_ |
| Stop | day's high | Tight short stop | _to fill_ |
| Direction | Short-only | _to fill_ |

---

## Observation log

Append dated observations as detectors run on live data:

```
### YYYY-MM-DD — <detector name>
- Symbol(s): ...
- Hit rate (true positive / total fired): X / Y
- False positive themes: ...
- Suggested adjustment: ...
```
