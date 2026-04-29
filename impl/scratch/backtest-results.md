# Backtest results scratchpad

Historical replay outcomes for each detector and each LLM-assisted variant. Populated by Phase 23 (`tracker_backtest`) and updated whenever calibration changes.

Use this to:
- Validate detector hit rates before relying on them live.
- Compare LLM-ranked top-N vs naive ranking.
- Identify regime conditions where a detector underperforms.

---

## Methodology

- **Universe:** initially the symbols already in the tracker DB; expanded later to S&P 500 + Russell 2000 if needed.
- **Window:** rolling 90 days backward from each backtest start; document the window in each result.
- **Hit definition:**
  - Long setup "hit" = price reaches 2R target before stop within 20 trading days.
  - Short setup "hit" = mirror.
- **Costs:** assume `0.05%` round-trip slippage + commissions; tweak in Phase 23.
- **LLM replay:** Phase 23 replays cached `llm_calls` rows when available to avoid re-billing.

---

## Detector hit rates (fill in as backtests run)

```
### YYYY-MM-DD — <detector> — window <start>..<end>
- Total signals: N
- Hits (2R reached): X (X/N = Y%)
- Stops hit: ...
- Mean R achieved: ...
- Median time-to-target: ... days
- Notable false positives: ...
- Configuration: <link to detector-calibration.md row>
```

---

## LLM-ranking lift

Compare the daily ranker's top-5 to the naive top-5 (by `conviction_signal` alone):

```
### YYYY-MM-DD — Ranker v<N> over <window>
- Naive top-5 hit rate: X%
- LLM top-5 hit rate: Y%
- Lift: (Y - X) percentage points
- Interpretation: ...
```

---

## Open questions for live operation

_to fill in once Phase 23 produces real numbers_:

- Which detectors are reliably profitable enough to trust unattended?
- Does the LLM ranker improve on the naive signal sufficiently to justify the daily Sonnet call?
- Are there market regimes where a detector should be auto-disabled?
