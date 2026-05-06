# Phase 8 — Portfolio risk view + concentration gate

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-06)

**Depends on:** 1

**Goal:** Make portfolio-level state legible pre-trade. Today every setup is evaluated in isolation; a 4th semis long carries no signal. After this phase, the system shows total open dollar-risk, concentration by sector + factor, and refuses (or warns on) setups that breach configured limits.

## Files

- New: `src-tauri/src/services/portfolio_risk/mod.rs` — `PortfolioRiskService::snapshot() -> PortfolioRisk`.
- New: `src-tauri/src/services/portfolio_risk/exposure.rs` — Computes sector + factor exposure from open positions, joined with `bracket_groups.stop_order_id` for "if all stops hit" dollar-risk.
- New: `src-tauri/src/services/portfolio_risk/sector_map.rs` — `symbol → sector` mapping. Reuses fundamentals provider's sector field; falls back to a small static SP500-sector JSON for missing symbols.
- New: `src-tauri/src/services/portfolio_risk/factors.rs` — Simple factor exposures: momentum (12-1m return percentile), value (P/E percentile), size (market-cap bucket). Bucketed coarsely; no factor model fitting.
- New: `src-tauri/src/services/portfolio_risk/concentration_gate.rs` — `ConcentrationGate::check(&NewSetup) -> GateResult`. Returns `pass | warn | block`.
- Touches: `src-tauri/src/services/tracker_runner/` — Run gate before persisting `SetupCandidate`. `block` writes `skipped_reason: ConcentrationBlocked`. `warn` persists normally with a `gate_warning` annotation.
- Touches: `src-tauri/src/storage/migrations/` — `portfolio_snapshots` table: `at`, `nlv_cents`, `total_dollar_risk_cents`, `exposures_json`. `setups.gate_warning TEXT`.
- New: `src-tauri/src/ibkr/commands/portfolio_risk.rs` — `portfolio_risk_snapshot`, `portfolio_risk_history`, `concentration_get_config`, `concentration_set_config`.
- New: `src/features/portfolio/components/RiskSnapshot.tsx` — Top-of-screen card: open dollar-risk, NLV %, sector exposure bar, "if all stops hit" P&L.
- New: `src/features/portfolio/components/ExposureMap.tsx` — Sector × factor heatmap.
- New: `src/features/tracker/components/GateWarningBanner.tsx` — In-modal banner during P3's TakeSetup flow if gate is `warn` or `block`.
- New: `src/shared/api/portfolioRisk.ts`.

## Tauri commands exposed

| Command | Purpose |
|---|---|
| `portfolio_risk_snapshot` | Current portfolio risk view. |
| `portfolio_risk_history` | Time-series of total dollar-risk and concentration. |
| `concentration_get_config` / `set_config` | Limits: max % NLV in single name / single sector / single factor; max total open dollar-risk. |

## Reuse

- P1 `EquitySnapshot` for NLV.
- P3 `bracket_groups` for stop prices on open positions (`if-all-stops-hit` calc).
- Existing positions service in `ibkr` for open-position list.
- Fundamentals provider for sector field.
- Existing `EventEmitter` to publish `PortfolioRiskChanged` (for live dashboard refresh).

## Decisions to make in this phase

- **Default concentration limits.** **Decision:** max 5% NLV dollar-risk in a single sector; max 1.5% NLV dollar-risk in a single name; max 10% NLV total open dollar-risk; max 4 concurrent positions in same factor bucket. All overridable.
- **Gate severity ladder.** **Decision: 80% of limit = `warn` (banner shown but trader can proceed without explicit override). 100% of limit = `block` (requires `gate_override_reason`).** Records both.
- **Snapshot cadence.** **Decision: recompute on `executions` event AND every 60 seconds.** Cached between recomputes.
- **Factor model.** **Decision: keep coarse this phase.** Three factors only (momentum, value, size). Sector is the high-information cut. Anything finer requires P10 to refit.
- **Pre-P3 brackets without recorded stops.** Some legacy positions may have stops set manually in TWS that we don't see. **Decision: when stop unknown, assume worst-case stop = 5% below entry; surface "stop estimated" annotation. Trader can manually attach stop via `bracket_attach_after_fact` (small new command).**
- **Cross-account.** **Decision: this phase is single-account. Multi-account aggregation deferred.** Hard-coded for current `AccountReader`.

## Exit criteria

- `cargo test portfolio_risk::` passes: exposure math (sector aggregation, dollar-risk weighting), gate severity ladder, snapshot recompute on event.
- Integration test: open a fixture position with bracket → snapshot shows correct dollar-risk; add a second concentrated position → gate fires `warn`; third → `block`.
- Frontend renders RiskSnapshot card with live values; ExposureMap colors correctly.
- TakeSetupModal shows gate banner when applicable; override flow records `gate_override_reason`.
- Concurrent stress: 100 rapid `executions` events do not corrupt the snapshot cache (single-flight recompute).
- Pre-commit clean.

## Gotchas

- **Sector classification gaps.** Smaller / non-US tickers may not have sector in fundamentals. Fall back gracefully; do not block setup just because sector unknown — log a warning.
- **"If all stops hit" math.** Sum of dollar-risks per position assumes independence. Document that this overstates loss in highly correlated markets (everything stops out together) and understates in mean-reverting (something is breaking).
- **Stop-distance changes intraday.** A trail bump (P7) reduces dollar-risk. Snapshot must be recomputed on `BracketRevised` events too.
- **Factor bucketing on small float.** Microcaps with no analyst coverage have NaN value/momentum. Bucket as "unknown" — never silently put them in the wrong bucket.
- **Concentration limits coupled to sizing.** P1 sizing already caps per-position notional; P8 caps per-sector dollar-risk. Both can fire on the same setup; gate result must surface which one bound.
- **Override audit.** Gate overrides go into the same `gate_overrides` table from master.md cross-phase verification. Trader-profile rollup shows override frequency.
- **Frontend layout creep.** RiskSnapshot is top-of-screen. Resist adding 12 sub-metrics; the screen must be readable in 1 second.
- **Don't compute factors live for every snapshot.** Cache factor membership per symbol with a 7-day TTL; refresh in background.
