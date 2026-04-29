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
| Lookback for high | 20 trading days | Standard "one month" breakout convention | _to fill in Phase 07_ |
| Volume multiple | 1.5× 20-day avg | Common breakout volume confirmation | _to fill_ |
| RSI(14) ceiling | 80 | Avoid already-extended names | _to fill_ |
| ATR period for stop | 14 | Standard | _to fill_ |
| Stop distance | min(swing_low_10, trigger − 1×ATR) | Tight but not micro-stop | _to fill_ |
| Targets | 2R, 3R | Risk profile (disciplined swing) | _to fill_ |
| Direction | Long-only (Phase 07) | Bear-side breakouts handled by parabolic-short detector instead | _to fill_ |

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
