# Phase 4 — Universe expansion + candidate staging

> Part of [Quantum Kapital → Autonomous Researcher](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-02)

**Depends on:** Phase 1 (MCP read), Phase 2 (MCP write — `promote_candidate`)

**Goal:** Decouple "scanner finds something" from "ticker enters watchlist." Add a staging layer (`candidate_universe`) the agent can browse and selectively promote. Expand scanner profiles.

## Files

- New migration: `candidate_universe(symbol, source, score, reason_md, first_seen, last_seen, decay_at, promoted_at NULL)`
- Touches: `src-tauri/src/services/auto_scanner/mod.rs` — write to `candidate_universe` instead of (or in addition to) directly promoting via `TrackerService::add_ticker`
- New: `src-tauri/src/services/candidate_promoter/mod.rs` — promotion logic (score threshold, agent action, decay rules)
- Touches: `src-tauri/src/config/settings.rs` — new scanner profiles (gap-and-go, sentiment-surge, earnings-mover) and promotion thresholds
- New MCP tools (extend `mcp/tools/reads.rs` and `writes.rs`):
  - `get_candidates(filter)` — by source, score range, recency
  - `promote_candidate(symbol, reason)` — agent moves it into watchlist
- New UI: `src/features/scanner/` extended — candidate browser, manual-promote button

## New scanner profiles

- **`top_pct_gainers`** — already configured, ensure live
- **`top_pct_losers`** — exists, may need to enable
- **`unusual_volume`** — IBKR scan code `HOT_BY_VOLUME` or similar
- **`gap_and_go`** — pre-market gap > 3% with above-avg volume
- **`breakout_setups`** — price breaking 20-day high
- **`sentiment_surge`** — joins `social_sentiment` (Phase 3), surfaces tickers with mention spike vs 7-day baseline
- **`earnings_movers`** — fundamentals-driven (Alpha Vantage upcoming earnings calendar)

## Reuse

- `scan_one_shot` in `src-tauri/src/ibkr/client/streams.rs` — already supports configurable scan codes + filters.
- `AutoScannerService` orchestration loop (gets refactored to write candidates instead of promoting directly).
- `MarketScanner` trait seam for testability.

## Decisions to make in this phase

- **Promotion strategy.** Score threshold (auto-promote) vs explicit agent action (every promotion is reasoned). Default: hybrid — auto-promote at high score, agent-only at medium, ignore below. Tune in shadow mode.
- **Decay window.** Default 7 days, configurable per-source. Sentiment surges decay faster (24-48h); fundamentals-driven candidates persist longer.
- **Cross-source dedup.** Same ticker hits from 3 sources → single row with merged score, or separate rows? Single row with `sources JSON` field is cleaner.

## Exit criteria

- Morning candidate set populated from 5+ sources (multiple IBKR scans + sentiment surges + earnings movers).
- Browseable in UI candidate view with source provenance + score.
- Agent can call `get_candidates(filter)` and `promote_candidate(symbol, reason)`.
- Decay job runs daily; expired candidates drop without manual cleanup.

## Gotchas

- **Scanner output volume.** "Top % gainers" can return 50+ tickers; without filtering, the candidate table grows fast. Apply price/volume floors + market-cap minimums.
- **Sentiment-surge profile is the cross-domain one.** It joins `social_sentiment` (Phase 3) with a baseline. Make sure the SQL is index-friendly.
- **Migration order.** If Phase 3 ships after Phase 4, the sentiment-surge profile needs a feature flag.
