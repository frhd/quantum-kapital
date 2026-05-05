# Phase 12 — Options awareness + multi-timeframe confluence (PUNTED)

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** punted

**Depends on:** 6, 7, 9

**Goal (deferred):** Two adjacent capabilities pulled out of the critical path because they're large and gated on evidence from earlier phases:

1. **Options-aware setups.** Episodic-pivot on news gaps is the canonical IV-crush situation. Equity-only directional trades leave structured-trade edge on the table.
2. **Multi-timeframe confluence.** Intraday triggers gated by daily / weekly trend alignment. The current detectors are single-timeframe and may be taking trades against larger-frame context.

## When to schedule (un-punt)

Pull this phase in when **all three** become true:

1. **P6 evidence:** episodic-pivot backtest shows realized R is consistently below directional-equivalent IV-crush realized R for the same setups, OR multi-day setups show clear weakness without daily-trend gate.
2. **IBKR data tier supports options chain pulls** without breaking the rate budget (`get_data_tier` reports an options-eligible tier).
3. **The first 11 phases are stable in production for at least 8 weeks** — enough trades through the new system to know real Sharpe before adding scope.

If any of those is missing, P12 stays parked.

## Sketch (do not implement until un-punted)

### Options module

- New: `services/options_chain/` — IBKR chain pulls, IV surface caching.
- New: `strategies/structured/` — Detector outputs become `StructuredTrade` (e.g., short put credit spread, calendar) instead of `OrderIntent`.
- Touches: `services/order_ticket/` — Multi-leg bracket submission.
- Touches: `services/risk_engine/` — Sizing for spreads (defined max-loss).
- Touches: `services/portfolio_risk/` — Greeks aggregation.
- Touches: `services/event_calendar/` — Earnings windows become "long IV" opportunities, not skip-trigger.

### Multi-timeframe confluence

- Touches: `strategies/context.rs` — `MarketContext` gains daily/weekly bars per symbol regardless of detector frame.
- Touches: each detector — declare confluence rules (e.g., breakout 15m only with daily trend up).
- Touches: P6 backtester — replay confluence-aware versions and compare.

### Hard invariants still apply

- Surveillance-only: structured trades require human confirmation, same as equity brackets.
- LlmService budget: options chain summarization (if any) routes through ledger.
- Mock-friendly trait seam for IBKR options paths.
- All audit on writes; no shims.

## Open questions

These move to `QUESTIONS.md` Phase 12 section when this phase is un-punted, not before:

- Spread sizing: what does "1R" mean for a defined-risk spread? Probably max-loss.
- IV surface staleness: the chain ages within minutes during fast tape; what's the freshness gate?
- Greeks aggregation in portfolio risk: do we add to existing exposure axes (sector, factor) a delta-equivalent + vega-equivalent summary, or split out an Options panel?
- Options-leg cancellation on partial-fill: IBKR semantics differ from equities; needs paper testing before live.

## Why this is punted

The first 11 phases get the system from "surveillance + journaling" to "deterministic risk-aware swing-trading." That's already a 12-week program. Adding options + MTF on day 1 splits attention. Earnings-driven directional gain capture from the existing detectors is enough to be measurable; once measured (P4 attribution, P6 backtest), the case for options either makes itself or doesn't.
