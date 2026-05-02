# Phase 5 — Cutover: composite as default + AV guardrails + tracker invariant test

> Part of [Alpha Vantage strip-out: manual MCP fundamentals + IBKR news](master.md). See index for invariants.

**Status:** done (commit e201093, 2026-05-02)

**Depends on:** 4

**Goal:** Make `CompositeFundamentalsProvider` the unconditional production path. Add the defensive guardrails on the AV branch so the user's *"don't spam AV on duplicate calls"* directive is enforceable in code. Add the tracker-doesn't-read-fundamentals invariant test (Hard Invariant #6). End-state: every `get_fundamentals` and `analysis.rs` call goes through the composite; AV is reached only when the manual store is empty AND the cache misses AND daily/per-symbol budgets allow; tracker compile-time can't accidentally pull fundamentals into a sweep.

This is a small phase by design. Phase 4 already wired the composite into `lib.rs`; this phase verifies the wiring under realistic call patterns and makes the invariants enforceable. There is no soak comparison and no shadow provider — the AV adapter survives Phase 5 and continues to serve the fallback path indefinitely.

## Files

- New: `src-tauri/src/services/fundamentals_provider/av_call_ledger.rs` — daily AV call counter. Soft cap at 20, hard cap at 25. In-memory `Mutex<HashMap<NaiveDate, u32>>` with daily rollover; persisted to a new `av_call_ledger(date PRIMARY KEY, count INTEGER)` SQLite table at every increment so a process restart doesn't reset the count. Per-symbol per-day cap (1 fetch per symbol per day) tracked in a sibling `Mutex<HashMap<(NaiveDate, String), u32>>`.
- New: `src-tauri/migrations/V<next>__av_call_ledger.sql` — refinery migration:
  ```
  CREATE TABLE av_call_ledger (
    date TEXT PRIMARY KEY,
    count INTEGER NOT NULL,
    updated_at TEXT NOT NULL
  );
  CREATE TABLE av_per_symbol_ledger (
    date TEXT NOT NULL,
    symbol TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (date, symbol)
  );
  ```
- New: `src-tauri/src/services/fundamentals_provider/av_call_ledger_tests.rs` — soft-cap warn test, hard-cap refusal test, per-symbol cap test, daily-rollover test, restart-survives test.
- Touches: `src-tauri/src/services/fundamentals_provider/composite.rs` (from Phase 4) — wire the ledger into the AV branch. Order: check per-symbol cap → check daily hard cap → log warn if past soft cap → call AV → increment ledger on success. On `DailyBudgetExhausted` or `PerSymbolBudgetExhausted`, return cached/stale data if available, else `FundamentalsError::DailyBudgetExhausted` / `FundamentalsError::PerSymbolBudgetExhausted`.
- Touches: `src-tauri/src/services/fundamentals_provider/mod.rs` (from Phase 3) — add `DailyBudgetExhausted { hit_count: u32 }` and `PerSymbolBudgetExhausted { symbol: String }` to `FundamentalsError`. The Phase 3 plan said these would be reachable here.
- New: `src-tauri/tests/integration/tracker_no_fundamentals.rs` — compile-time / link-time test that the tracker pipeline does not depend on `FundamentalsProvider`. Implementation options (pick one in the phase):
  - **(a) Crate-graph grep:** test that runs `cargo metadata` (or just greps `tracker_runner/`, `strategies/`, `eod_scheduler/`, `intraday_scheduler/` source files) and asserts zero matches for `FundamentalsProvider`, `fetch_fundamental_data`, `FundamentalData`. Fast, fragile under refactor.
  - **(b) Sentinel symbol:** add `#[deny(unused_imports)]` on a sentinel module that pulls in `FundamentalsProvider` only if mistakenly imported into the tracker; relies on conditional compilation. Cleaner, more invasive.
  - Default: (a). Document in the test file why this exists (Hard Invariant #6).
- New: `src-tauri/tests/integration/composite_provider_e2e.rs` — end-to-end against a `MockHttp` AV transport + in-memory `Db`. Asserts: empty store + AV mock returns X → `fetch` returns X + ledger incremented. Manual `set_fundamentals` writes Y → subsequent `fetch` returns Y + AV mock asserts zero new calls. Manual store has Y, then `clear` it → `fetch` falls back to AV cache, then AV. Daily ledger pre-populated to 25 → `fetch` for unseen symbol returns `DailyBudgetExhausted`.
- Touches: `src-tauri/src/lib.rs` — pass the ledger handle (and Db pool for persistence) to `CompositeFundamentalsProvider` construction.
- Touches: `loop/plan/QUESTIONS.md` — log any surprises observed during the cutover (e.g., a place where AV is being called more often than expected, or a UI symbol that depends on a field the manual store doesn't populate).

## Reuse

- Phase 4 `CompositeFundamentalsProvider`, `ManualFundamentalsStore`, `set_fundamentals` MCP tool — all in place.
- Phase 1 stale-cache fallback (`CacheService::read_ignoring_ttl`) — composite already uses it for the AV branch.
- Existing `Db` pool (r2d2 SQLite) — ledger borrows it.
- `FakeFundamentalsProvider` from Phase 3 — used in any test that doesn't care about composite internals.

## Decisions to make in this phase

- **Ledger persistence.** SQLite-backed (survives restart, ~30 ms write per call) vs. in-memory only (resets on restart, faster). Default: SQLite-backed — restart-during-sweep without ledger persistence would let a misbehaving caller blow through the daily cap. Cost is negligible.
- **What does "per-symbol per day" measure?** All AV API calls vs. only AV API calls that returned data vs. all `composite.fetch(symbol)` invocations. Default: AV API calls (whether successful or rate-limited) — protects the quota even when AV is degrading.
- **Behavior on `DailyBudgetExhausted` with no cache.** Return error vs. return empty `FundamentalData` vs. return last-known-stale-from-any-symbol. Default: return error. The UI shows "fundamentals temporarily unavailable; paste manually via Claude or wait for tomorrow." Honest > silent.
- **Soft-cap log level.** `warn` vs. `info`. Default: `warn` — soft-cap is a trip-wire; if it fires regularly something is wrong.
- **Tracker invariant test enforcement strictness.** Should the test grep `services/` broadly, or only the tracker-adjacent modules? Default: only tracker-adjacent (`tracker_runner/`, `strategies/`, `eod_scheduler/`, `intraday_scheduler/`, `news_interpreter/`, `thesis_generator/`, `decay_watcher/`). Anything *outside* those modules is allowed to read fundamentals.
- **Telemetry on cutover day.** Add a one-off log line each time the morning sweep starts noting which provider sources are populated and how recent the manual store entries are. Helps debug "where did this number come from?" post-mortem.

## Exit criteria

- `cargo test` and `pnpm test:run` clean.
- New tracker-no-fundamentals invariant test passes; intentionally inserting `use crate::services::fundamentals_provider::FundamentalsProvider;` into `tracker_runner/mod.rs` makes it fail (verified manually before commit).
- New composite e2e test passes including the daily-budget exhaustion path.
- Manually verified end-to-end (record in commit body):
  1. Open analysis screen for a symbol not in manual store; confirm AV is hit (one increment in `av_call_ledger`).
  2. From Claude Code: `set_fundamentals(symbol="<that symbol>", ...)`. Confirm AV cache row is gone after the call.
  3. Re-open the analysis screen for the same symbol; confirm zero new AV increments and the manual data renders.
  4. Pre-populate ledger to 25 (manual SQL). Open analysis screen for a fresh symbol; confirm UI shows the budget-exhausted error string.
- Cross-phase tracer-bullet from `master.md § Cross-phase verification #1` passes.
- Pre-commit clean.

## Gotchas

- **`av_call_ledger` race conditions.** Two concurrent `fetch` calls may both pass the cap-check before either increments. SQLite `INSERT ... ON CONFLICT(date) DO UPDATE SET count = count + 1` in a single statement avoids the lost-update problem; verify the migration uses the right primary-key shape so this works.
- **Daily rollover at midnight.** The ledger key is `NaiveDate::today()` in the user's local timezone (or UTC?). Pick one and document. Default: user's local timezone (matches the morning-sweep cadence). UTC would shift the rollover into the user's evening, which is confusing.
- **AV cache key vs. ledger key.** AV cache is keyed per (symbol, endpoint); ledger is keyed per (date, symbol) and per (date). Don't conflate. Each AV call increments the per-symbol counter once per `composite.fetch(symbol)`, even though that triggers 3 endpoint calls under the hood (`overview`, `income`, `earnings` via `tokio::try_join!`). Decision: ledger increments once per `composite.fetch` (representing the operator's cost), not three times. Document this in the ledger doc-comment.
- **Per-symbol cap interacts badly with cache invalidation.** If the user calls `set_fundamentals` for symbol X (clearing AV cache), then later that same day clears the manual store for X, the next `fetch(X)` would fall through to AV — but per-symbol cap might have already been spent for X earlier. Decision: clearing the manual store does NOT reset the per-symbol counter. If the user wants to re-fetch from AV, they wait until tomorrow or accept the empty result.
- **Invariant test brittleness under refactor.** The tracker-no-fundamentals test (option `a`) is a string grep; renaming `FundamentalsProvider` would break it. Acceptable cost for the protection it gives. If renaming, update the test in the same commit.
- **Settings flag absence.** Resist adding a `disable_av_fallback: bool` flag. The composite always tries the manual store first; if AV is unhealthy, the per-symbol cap and daily cap protect the quota. There is no scenario where a global "no AV" flag is the right answer — if AV is genuinely dead, fix the AV adapter; don't add a feature flag to skip it.
- **Subscription lapse safety.** If `ALPHA_VANTAGE_API_KEY` is unset at runtime, the AV branch returns `FundamentalsError::Other("Alpha Vantage API key not configured")`. The composite still serves manual-store hits. This is acceptable post-Phase-3 (mock fallback removed); document in `CLAUDE.md`.
- **Don't delete the AV adapter at the end of this phase.** Phase 8 is the AV-news-deletion phase; AV fundamentals adapter survives it (Hard Invariant #9).
