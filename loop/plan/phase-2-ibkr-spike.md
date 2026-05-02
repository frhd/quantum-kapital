# Phase 2 — IBKR Reuters fundamentals spike (de-risk)

> Part of [Alpha Vantage → IBKR Reuters](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-02)

**Depends on:** none (independent of Phase 1; can run in parallel)

**Goal:** Confirm the IBKR Reuters Fundamentals path is reachable from this codebase before designing the trait abstraction. Outcomes: (a) `ibapi = "2"` exposes `req_fundamental_data` — or we have a documented Plan B; (b) the user's IBKR account has the Reuters Worldwide Fundamentals subscription enabled; (c) we have real XML fixtures for each `reportType` saved to disk so Phase 4's parser tests don't need TWS.

## Files

- New: `src-tauri/tests/fixtures/ibkr_fundamentals/AAPL_ReportSnapshot.xml`
- New: `src-tauri/tests/fixtures/ibkr_fundamentals/AAPL_ReportsFinSummary.xml`
- New: `src-tauri/tests/fixtures/ibkr_fundamentals/AAPL_ReportsFinStatements.xml`
- New: `src-tauri/tests/fixtures/ibkr_fundamentals/AAPL_RESC.xml`
- New: `src-tauri/src/bin/ibkr_fundamentals_spike.rs` — throwaway binary that connects to TWS, calls `req_fundamental_data` for AAPL on each `reportType`, writes the XML to the fixture paths above. Deleted at end of phase, or moved behind `#[cfg(feature = "ibkr-spike")]`.
- New: `loop/plan/notes/ibkr-fundamentals-xml.md` — short notes on the XML structure of each report type (top-level elements, where revenue / income / EPS / analyst estimates live), referenced by Phase 4 when writing parsers.

## Reuse

- Existing `IbkrClient` connection plumbing in `src-tauri/src/ibkr/client/` for the spike binary's TWS handshake.
- Existing TWS connection settings in `src-tauri/src/config/settings.rs` so the spike doesn't hardcode credentials.

## Decisions to make in this phase

- **Crate path:** if `ibapi = "2"` exposes `req_fundamental_data`, use it. If not, choose between (a) forking the crate, (b) writing a raw TWS-message wrapper, (c) switching to a different IBKR Rust crate. Document the decision in the phase exit notes.
- **Subscription confirmed?** Snapshot of TWS Account → Market Data Subscriptions showing Reuters Worldwide Fundamentals as active. If not subscribed, this phase blocks until the user subscribes.
- **Which symbols for fixtures.** AAPL is the default; consider adding one ADR (e.g., `BABA`), one small-cap, and one symbol with sparse fundamentals (e.g., `RDDT`) so Phase 4 parsers handle the common edge cases. Decide which extras are worth capturing now vs. later.

## Exit criteria

- The four AAPL fixtures exist on disk and contain non-empty Reuters XML (>1KB each, parseable by `xmllint --noout`).
- The spike binary runs to completion against a paper TWS account from a clean checkout: `cargo run --bin ibkr_fundamentals_spike --features ibkr-spike`.
- A short paragraph in `loop/plan/notes/ibkr-fundamentals-xml.md` for each `reportType`: top-level XML element, where the fields needed for `FundamentalData` (revenue history, EPS, analyst estimates, current ratios) live, any noted variations across symbols sampled.
- Crate-path decision recorded in this file under `## Decisions to make`: either "ibapi 2 supports it, use directly" or named alternative with rationale.
- Subscription status confirmed (screenshot or written confirmation in `QUESTIONS.md`).

## Gotchas

- **TWS pacing.** `req_fundamental_data` is rate-limited by TWS itself (typically 60 / 10min for fundamentals). Don't call all four reportTypes in a tight loop — the spike binary should sleep ~2 seconds between requests.
- **Subscription denial returns a TWS error message, not an HTTP error.** The spike must distinguish "no subscription" (error code 430) from "no data for symbol" (200) and "TWS not connected" — these become the `FundamentalsError` variants in Phase 3.
- **Paper account vs. live.** Reuters Fundamentals subscription typically applies to live accounts; paper may return empty or refuse. If using paper, expect to switch to live for the spike or test on a live read-only flow.
- **XML version drift.** Save the fixtures with the date of capture in a comment header so Phase 4 parsers can be re-fixtured deliberately.
- **Spike code is throwaway.** Don't let it become "production-ready" — it exists to capture fixtures, not to be a permanent provider. Phase 4 writes the real implementation from scratch with TDD.
