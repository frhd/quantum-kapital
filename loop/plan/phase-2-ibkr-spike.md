# Phase 2 — IBKR Reuters fundamentals spike (de-risk)

> Part of [Alpha Vantage strip-out: manual MCP fundamentals + IBKR news](master.md). See index for invariants.

**Status:** abandoned (2026-05-02 — see "Why abandoned" below)

**Depends on:** none (was independent of Phase 1)

## Why abandoned

The spike's premise — that `req_fundamental_data` was a viable replacement for AV fundamentals — was falsified by two compounding findings on 2026-05-02:

1. **The TWS API method is officially DEPRECATED.** IBKR's own docs at `interactivebrokers.github.io/tws-api/fundamentals.html` warn: *"this interface still works as of now, but it is possible that IB will stop honoring these requests in the future."* `EClient.reqFundamentalData` is marked Legacy/DEPRECATED in the API reference. Throughout 2024-2025, multiple users (`twsapi@groups.io` thread "Is reqFundamentalData broken?" from 2024; `quantbelt/ib_fundamental` Discussion #11, Oct 2024 → Mar 2025; Issue #12 reply from maintainer `gnzsnz`) report `ReportsFinStatements` and `RESC` failing intermittently or persistently for entitled accounts. Maintainer of `ib_fundamental` on Issue #12, March 2025: *"IBKR API is been down for a few weeks now. There is nothing that i can do."*
2. **This account hits error 10358 "Fundamentals data is not allowed"** for every reportType, despite IBIS Research Platform (Fee Waived) being active. Verified 2026-05-02 via Python `ibapi` capture script against TWS on port 4004. **IBIS feeds the TWS UI Financials tab but does NOT enable the API path** — different entitlements. The historical "Reuters Worldwide Fundamentals" line item that used to enable the API is **not on the GFIS Subscriptions page** for this account tier; IBKR has been winding down API entitlements for retail since the Refinitiv → LSEG transition.

The IBKR Web API explicitly removed fundamentals tags (P/E, EPS, Market Cap, Dividend Yield, Beta — all marked deprecated). The replacement in IBKR's universe is `reqWshMetaData`/`reqWshEventData` from Wall Street Horizon — but this is **events only** (~$250/mo for the Enchilada bundle), not financial statements.

Building Phase 4's XML parsers around a deprecated, degrading API on an account that can't reach it would be building on sand. The pivot decided 2026-05-02 (see master.md Context) replaces this entire arc with: MCP `set_fundamentals` write tool (LLM-mediated manual paste) + AV fundamentals adapter retained as opportunistic fallback. The crate-path decision (fork `ibapi`) is also abandoned — no fork is needed if we're not calling `req_fundamental_data` at all.

## What we kept from this phase

- The `ibapi` crate-path **investigation** stays valid for Phase 6/7: published `ibapi = "2.11.x"` already exposes news APIs (`news_providers`, `historical_news`, `news_article`) — Phase 6 confirmed this independently. No fork is needed for news either.
- The Python `ibapi` setup (uv-installed, capture script blueprint) is reusable — it was the harness that produced the 10358 evidence. Keep `loop/plan/notes/ibkr-fundamentals-xml.md` as a historical record.
- The user's **TWS API setup** (port 4004, "Enable ActiveX and Socket Clients" toggled on) is intact and useful for Phase 6 news capture.

## Original goal (preserved for context)

Confirm the IBKR Reuters Fundamentals path is reachable from this codebase before designing the trait abstraction. Outcomes: (a) `ibapi = "2"` exposes `req_fundamental_data` — or we have a documented Plan B; (b) the user's IBKR account has the Reuters Worldwide Fundamentals subscription enabled; (c) we have real XML fixtures for each `reportType` saved to disk so Phase 4's parser tests don't need TWS.

The original goal sections below remain for historical reference; none of them are actionable.

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

- **Crate path:** **DECIDED 2026-05-02 — fork `ibapi` and add
  `Client::fundamental_data` for the Phase 4 production provider; use
  the official Python `ibapi` package for Phase 2 fixture capture.**
  Rationale and full investigation in
  `loop/plan/notes/ibkr-fundamentals-xml.md` § "Crate-path decision".
  Short version: `ibapi = "2.11.x"` does not expose
  `req_fundamental_data` (verified against both the locally-resolved
  source and the upstream `main` branch) and its `MessageBus` is
  `pub(crate)`, so a raw-message wrapper is not viable from a
  downstream crate. Forking is the smallest patch surface; vendor via
  `[patch.crates-io]` and submit the patch upstream in parallel.
- **Subscription confirmed?** **Pending user confirmation** — see
  `QUESTIONS.md § P2`. Cannot be resolved autonomously. Capture and
  subscription confirmation are the only remaining work for this
  phase.
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
