# Cross-phase questions / open issues

Logged during phase execution per `loop/plan/master.md`. Each entry: who
found it, when, and the safer interpretation taken so the loop could
continue. Resolve and prune as phases progress.

---

## P1: `decay_watcher::tests::respects_budget_kill_switch` — pre-existing failure on `main`

- **Found:** Phase 1, 2026-05-02
- **Symptom:** `cargo test --lib services::decay_watcher::tests::respects_budget_kill_switch`
  panics with `MockHttp queue exhausted` at `src/services/decay_watcher/tests.rs:59`.
- **Reproducible on `main`** (verified with `git stash` before Phase 1
  changes). Not introduced by the AV burn-fix work; the failing test
  has nothing to do with the AV path.
- **Safer interpretation:** treat as an unrelated test-suite regression
  on `main`; do not block Phase 1 on it. The Phase 1 exit criteria say
  `cargo test` green; the underlying reason here is in the LLM decay
  watcher mock setup, which is orthogonal.
- **TODO:** investigate independently (likely a stale fixture / missing
  enqueue after a recent prompt change). Phase 1 commits do not touch
  `decay_watcher/`.

---

## P2: Phase 2 abandoned — IBKR fundamentals migration arc replaced with manual MCP path

- **Found:** Phase 2, 2026-05-02
- **Status:** RESOLVED BY PIVOT (2026-05-02). Phase 2 is `abandoned`.
  See `phase-2-ibkr-spike.md` for the full rationale.
- **Investigation that triggered the pivot:**
  - `IBIS Research Platform` (Fee Waived) is active on the account
    and feeds the TWS UI Financials tab.
  - `reqFundamentalData("AAPL", reportType)` returns
    **error 10358 "Fundamentals data is not allowed"** for every
    reportType. Verified 2026-05-02 via the Python `ibapi` capture
    script (`/tmp/capture_av.py`) against TWS on port 4004.
  - **IBIS feeds the UI but NOT the API path** — different
    entitlements, confirmed by IBKR support patterns observed in
    `twsapi@groups.io` and `quantbelt/ib_fundamental` Issue #12 +
    Discussion #11 (2024-2025).
  - **The `reqFundamentalData` API itself is officially DEPRECATED**
    in IBKR's TWS API docs. `EClient.reqFundamentalData` is marked
    Legacy/DEPRECATED. Multiple users report `ReportsFinStatements`
    and `RESC` failing intermittently for entitled accounts since
    March 2025; maintainer of `ib_fundamental` (Mar 2025): *"IBKR
    API is been down for a few weeks now. There is nothing that i
    can do."*
  - The historical "Reuters Worldwide Fundamentals" GFIS line item
    does not appear for this account tier; IBKR has been winding
    down API entitlements for retail since the Refinitiv → LSEG
    transition. The IBKR Web API explicitly removed the equivalent
    fundamentals tags. The only forward-looking IBKR path
    (`reqWshMetaData` / Wall Street Horizon Enchilada Pro) is
    events-only at ~$250/mo — not financial statements.
- **Pivot decided 2026-05-02:** drop the IBKR fundamentals
  migration entirely. Replace with a new manual-paste path: MCP
  `set_fundamentals` write tool (LLM-mediated extraction from
  user-pasted text) + AV fundamentals adapter retained as
  opportunistic fallback with hard guardrails (daily ledger
  20 soft / 25 hard, per-symbol per-day cap, manual-write
  invalidates AV cache).
- **Relevant evidence (research dated 2026-05-02):**
  - Tracker pipeline + every strategy verified via grep to NOT
    read fundamentals at runtime. The 100-ticker morning sweep is
    news-driven; fundamentals are user-explicit only (analysis UI
    + MCP `get_fundamentals`). AV's 25/day cap is therefore not
    under sweep pressure. This is what makes the manual-paste
    workflow viable for a single-user app.
  - `quantbelt/ib_fundamental` Python wrapper uses the same
    deprecated API; it would not bypass the entitlement gap.
  - Alternative providers considered: Financial Modeling Prep
    ($22/mo Starter, best 1:1 fit for `FundamentalData` shape),
    Polygon.io (weak on fundamentals), EDGAR/SEC XBRL (free but
    no analyst estimates), AV Premium ($50/mo). None chosen — the
    user accepted the manual-paste tradeoff to avoid yet another
    vendor relationship.
- **What replaces Phases 2/3/4/5 (was IBKR fundamentals arc):**
  - Phase 3 (kept, retargeted): `FundamentalsProvider` trait + AV
    adapter. No IBKR provider. No `fundamentals_source` flag.
  - Phase 4 (renamed `phase-4-mcp-fundamentals.md`):
    MCP `set_fundamentals` write tool + `ManualFundamentalsStore`
    (SQLite) + `CompositeFundamentalsProvider`
    (manual → AV cache → AV API).
  - Phase 5 (kept, retargeted): cutover + AV daily-call ledger +
    per-symbol cap + tracker-doesn't-read-fundamentals invariant
    test.
  - Phase 8 (scope reduced): deletes only AV news code; AV
    fundamentals adapter, rate limiter, cache directory, and
    `ALPHA_VANTAGE_API_KEY` env var all retained as fallback infra.
- **No further user action required for P2.** The TWS setup (port
  4004, API enabled) the user established here is reusable by
  Phase 6 news capture (P3 below).
- **What's still blocked:** Two of Phase 2's exit criteria still
  need a live TWS / IB Gateway running locally:
  1. The four `AAPL_*.xml` fixtures under
     `src-tauri/tests/fixtures/ibkr_fundamentals/` (>1KB each).
  2. The Python capture script run-to-completion against a paper or
     live TWS at `127.0.0.1:7497`.
- **Why this is human-in-the-loop:** The /loop session has no TWS
  instance and cannot subscribe to Reuters on the user's behalf. The
  capture-script blueprint in
  `loop/plan/notes/ibkr-fundamentals-xml.md` is ready to run as soon
  as the user is at the desk with TWS up.
- **Crate-path decision recorded autonomously:** See the same notes
  file. `ibapi = "2.11.x"` does **not** expose `req_fundamental_data`
  and the `MessageBus` is `pub(crate)`, so we cannot synthesise a
  raw outgoing frame from outside. Decision: fork `ibapi` and add
  `Client::fundamental_data` for Phase 4; use the official Python
  `ibapi` package for Phase 2 fixture capture. The Rust spike
  binary at `src-tauri/src/bin/ibkr_fundamentals_spike.rs` is a
  feature-gated stub pointing at the notes file until the fork
  exists.
- **Safer interpretation taken:** Phase 2 stays `in-progress` (not
  flipped to `done`) until the four fixtures land and the
  subscription is confirmed. Phases 3, 4, 5 all depend on Phase 2
  being `done`, so they are correctly blocked on the user. The /loop
  ends this iteration with `loop/BREAK` so we don't burn iterations
  re-discovering the same blocker.
- **Action items for the user (when TWS is up):**
  1. ✅ Subscription confirmed via IBIS Research Platform (2026-05-02).
  2. `pip install ibapi` (official PyPI package) and run the
     Python capture script from
     `loop/plan/notes/ibkr-fundamentals-xml.md` § "Capture script
     blueprint". It writes the four XML fixtures.
  3. Verify each fixture is non-empty and parseable
     (`xmllint --noout src-tauri/tests/fixtures/ibkr_fundamentals/*.xml`).
  4. Resume the /loop or kick a fresh iteration; Phase 2 can then
     flip to `done` and Phase 3 unblocks.

---

## P4: Phase 5 manual end-to-end verification — deferred until user is at the UI

- **Found:** Phase 5, 2026-05-02
- **What's deferred:** Step 5 of Phase 5's exit criteria asks for a
  4-step manual check inside the running app:
  1. Open analysis screen for a symbol not in the manual store →
     confirm AV is hit (one increment in `av_call_ledger`).
  2. From Claude Code: `set_fundamentals(symbol="<that symbol>", ...)`
     → confirm the AV cache row is gone after the call.
  3. Re-open the analysis screen for the same symbol → confirm zero
     new AV increments and the manual data renders.
  4. Pre-populate the ledger to 25 (manual SQL) → open analysis for a
     fresh symbol → confirm UI shows the budget-exhausted error string.
- **Why this is human-in-the-loop:** every step requires the running
  Tauri app + a live `ALPHA_VANTAGE_API_KEY` + the React UI. The
  /loop session has neither.
- **Safer interpretation taken:** all automated checks (lib + integ
  tests, tracker invariant, composite e2e — daily/per-symbol cap
  exhaustion paths, stale-cache fallback, manual-write invalidates
  AV cache) are green. The behaviour is exercised by tests; the
  manual check is end-to-end smoke. Phase 5 flips to `done` on the
  basis of the automated coverage. The user can run the manual check
  next time the app is up; surface any divergence by amending this
  entry.
- **Pre-existing oddity surfaced and fixed in Phase 5:**
  `clear_fundamentals_cache` was using suffix `"income"` while the
  AV writer uses `"income_statement"`. Manual writes were silently
  skipping the income-statement cache row (Hard Invariant #8 hole).
  Centralised on `AV_FUNDAMENTALS_CACHE_SUFFIXES` and updated the
  Phase 4 unit test to match production cache keys.

---

## P3: Phase 6 spike capture is human-in-the-loop — TWS + at least one news subscription required

- **RESOLVED 2026-05-02 (later same day):** User ran the spike against
  port 4004 (paper Gateway). All three fixtures landed under
  `src-tauri/tests/fixtures/ibkr_news/` and exit criteria #1-#3 cleared:
  - **`news_providers.json`:** 8 active providers — `BRFG`
    (Briefing.com General), `BRFUPDN` (Briefing.com Analyst Actions),
    plus the full Dow Jones bundle: `DJ-N` (Global Equity Trader),
    `DJ-RT` (Trader News), `DJ-RTA/E/G` (Asia Pacific / Europe / Global
    Top Stories), `DJNL` (Newsletters). No Reuters Real-time News
    line on this account, but Briefing + DJ together easily cover the
    AAPL test universe. Cost has been baked into the existing IBKR
    Research Platform subscription — no incremental upgrade needed for
    Phase 7.
  - **`AAPL_historical.json`:** 50 headlines / 24h window, far above
    the ≥10 floor. Inline `{A:800015:L:en}` metadata blocks at the
    head of each headline (Apple's IBKR conid 265598 + locale tag) —
    the Phase 7 parser will need to strip these. Times are RFC3339
    UTC. `provider_code` per-article (DJ-N dominant in the spot-check
    sample).
  - **`AAPL_article_DJ_N_1e4fc3a3.json`:** `article_type: "Text"`,
    body is HTML-with-entities (`<p>...&apos;...</p>` chunks plus a
    closing disclaimer block). Confirms the Phase 6 gotcha — the
    Phase 7 parser must accept HTML and Base64 binary alike.
- **Found:** Phase 6, 2026-05-02
- **What's blocked:** Three of Phase 6's exit criteria need a live
  IBKR account with at least one news subscription enabled (Reuters
  Real-time News at minimum; Briefing.com / Dow Jones nice-to-have)
  and TWS / IB Gateway running locally on `127.0.0.1:7497`:
  1. The three fixtures under
     `src-tauri/tests/fixtures/ibkr_news/`:
     `news_providers.json`, `AAPL_historical.json`, and
     `AAPL_article_<id>.json` — all written by the spike binary.
  2. The Rust spike binary
     (`src-tauri/src/bin/ibkr_news_spike.rs`) run-to-completion
     against a paper or live TWS.
  3. Subscription confirmation (which providers are active, monthly
     cost, additions needed — recorded here when the user has
     completed the capture).
- **Why this is human-in-the-loop:** the /loop session has no TWS
  instance and cannot subscribe to news feeds on the user's behalf.
  The spike binary is wired up, compiles cleanly (`cargo check
  --features ibkr-spike --bin ibkr_news_spike` is green) and is
  ready to run as soon as the user is at the desk with TWS up.
- **Crate-path decision recorded autonomously:** See
  `loop/plan/notes/ibkr-news-shape.md` § "Crate-path decision".
  Short version: **the published `ibapi = "2.11.x"` already exposes
  `news_providers`, `historical_news`, `news_article`, and
  `news_bulletins` as public methods on `Client` (sync feature).
  No fork is needed for news.** The Phase 2 fork for fundamentals
  is orthogonal and unchanged. This is a meaningful win — Phase 7
  can build on the released crate directly.
- **Sentiment-loss audit completed autonomously:** See
  `loop/plan/notes/sentiment-loss-audit.md`. Every consumer of
  AV's per-article sentiment fields enumerated, every one of them
  resolves to "tolerate" — no consumer is structurally dependent
  on per-article scoring. Verdict path (`NewsInterpreter`) carries
  the load. Phase 7 will encode this as a regression test.
- **Safer interpretation taken:** Phase 6 stays `in-progress` (not
  flipped to `done`) until the three fixtures land and a
  subscription line lands here. Phases 7 / 8 depend on Phase 6
  being `done`, so they are correctly blocked on the user.
- **Action items for the user (when TWS is up):**
  1. Confirm at least one news subscription is active under
     TWS → Account → Market Data Subscriptions. Reuters Real-time
     News is the recommended minimum; Briefing.com (BRFG) and
     Dow Jones (DJ-N / DJNL) round out the verify-mix.
  2. Run the spike binary from the repo root:
     `cargo run --bin ibkr_news_spike --features ibkr-spike -- --port 7497`
     (use `7496` for live, `7497` for paper). It writes the three
     fixtures and prints the captured headline count.
  3. Verify fixtures are non-empty:
     `ls -la src-tauri/tests/fixtures/ibkr_news/` — expect at least
     `news_providers.json` and `AAPL_historical.json`. The article
     body file is named after the first article id and may be
     missing if the historical list comes back empty.
  4. Spot-check that ≥10 headlines came back over the 24h window
     (Phase 6 exit gate). If <5, expand the provider mix and
     re-run before declaring the phase done.
  5. Resume the /loop or kick a fresh iteration; Phase 6 can then
     flip to `done` and Phase 7 unblocks.

---

## P5: Phase 8 cutover landed; deletion gated on shadow soak

- **Found:** Phase 8 cutover, 2026-05-02
- **What changed:**
  - `default_news_source()` flipped from `"alpha_vantage"` to `"ibkr"`.
  - `lib.rs` match restructured to explicit arms — existing
    `settings.json` files with `news_source: "alpha_vantage"` still
    route to the AV adapter through the soak window.
  - New `shadow_av_news_comparison: bool` settings field (default
    `false`). When `true` AND an `ALPHA_VANTAGE_API_KEY` is
    configured, `ShadowingNewsProvider` wraps the IBKR provider and
    fires AV in the background, logging `shadow_news_comparison`
    spans with `coverage_ratio` + `material_gap` per call. AV
    failures during shadowing are swallowed (logged as
    `AV unavailable, no comparison`) so the IBKR path never blocks.
  - `tests/news_provider_parity.rs` — `#[ignore]`-gated live test
    over AAPL/AMD/DIS/TSM/RIVN. Run manually with
    `cargo test --test news_provider_parity -- --ignored --nocapture`.
- **What's blocked / human-in-the-loop:** the deletion commit
  (`AlphaVantageNewsProvider`, `services/financial_data_service/news.rs`,
  the news-side `news_source` flag, `ShadowingNewsProvider` itself)
  cannot land until ~2 weeks of clean shadow operation. Mechanically
  the user must:
  1. Set `shadow_av_news_comparison: true` in
     `~/.config/quantum-kapital/settings.json` (or via the UI when
     a settings panel exposes it).
  2. Run the morning sweep over the standard 100-ticker universe at
     least a few times across the soak window. Each sweep produces
     `shadow_news_comparison` log lines (one per symbol) with the
     coverage ratio.
  3. Spot-check `/tmp/qk-tauri.log` for `material_gap=true` lines.
     Per-symbol gaps below 80% over multiple runs need to be logged
     here before the deletion commit (per
     `phase-8-av-deletion.md` § "Decisions to make in this phase").
  4. Optionally run `cargo test --test news_provider_parity --
     --ignored --nocapture` against TWS once per week of soak as a
     point-in-time fixture.
- **Safer interpretation taken:** Phase 8 stays `in-progress` (not
  flipped to `done`). The deletion phase is mechanically gated on
  human confirmation that the soak is clean. Resume the /loop after
  ~2 weeks (target window: **2026-05-16 → 2026-05-23**) to land the
  deletion commit; if shadow logs surface a material gap, log
  affected symbols here first.
- **Pre-existing failure unchanged:** P1
  (`decay_watcher::tests::respects_budget_kill_switch`) still red
  on `main`. Cutover did not touch it.
