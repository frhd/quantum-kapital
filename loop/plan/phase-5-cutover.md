# Phase 5 — Cutover + AV deprecation

> Part of [Alpha Vantage → IBKR Reuters](master.md). See index for invariants.

**Status:** todo

**Depends on:** 4

**Goal:** Flip the default `fundamentals_source` to `"ibkr"`. Run a 100-ticker morning sweep against IBKR fundamentals, compare to AV output for the same symbols, ratify or reject the migration. After ~2 weeks of clean operation under shadow comparison, delete `AlphaVantageFundamentalsProvider`, the AV cache directory branch, and the AV-only env var requirement (for fundamentals — AV news still uses it).

## Files

- Touches: `src-tauri/src/config/settings.rs` — change default for `fundamentals_source` from `"alpha_vantage"` to `"ibkr"`. Existing settings files keep their stored value (no auto-migration).
- Touches: `src-tauri/src/lib.rs` — wire `IbkrFundamentalsProvider` as the default branch in the provider-construction match.
- New: `src-tauri/tests/integration/fundamentals_provider_parity.rs` — runs both providers against a 5-symbol fixture set, diff-asserts the fields. Live tests behind `#[ignore]` so CI doesn't need TWS.
- New: `src-tauri/src/services/fundamentals_provider/shadow.rs` — temporary shadow-comparison wrapper. Wraps the IBKR provider, fires the AV provider in the background, logs disagreements. Active only when settings include `shadow_av_comparison: true`. Deleted at the end of the soak window.
- Touches: `loop/plan/QUESTIONS.md` — log shadow-comparison findings (symbols + fields where IBKR and AV disagreed materially).

**After ~2-week soak (separate commit):**

- Delete: `src-tauri/src/services/fundamentals_provider/alpha_vantage.rs`.
- Delete: `src-tauri/src/services/fundamentals_provider/shadow.rs`.
- Touches: `src-tauri/src/services/financial_data_service/{overview,income,earnings}.rs` — these become unused on the news-only path; remove or mark unused. The news path doesn't reference them, so likely full deletion.
- Touches: `src-tauri/src/services/financial_data_service/mod.rs` — `fetch_fundamental_data` removed; struct keeps `fetch_news_sentiment`. Add a top-of-file comment pointing readers to `services/fundamentals_provider/` for fundamentals.
- Touches: `src-tauri/src/config/settings.rs` — `fundamentals_source` field collapses to a single value; consider deleting the field entirely or leaving as `String` for future flexibility.
- Touches: `src-tauri/src/lib.rs` — drop the AV branch.

## Reuse

- Existing `CacheService` for IBKR cache (Phase 4 already uses it).
- Phase 2 + Phase 4 fixtures stay under `tests/fixtures/ibkr_fundamentals/`.
- `services/fundamentals_provider/test_support.rs::FakeFundamentalsProvider` for any downstream tests that need provider injection.

## Decisions to make in this phase

- **Soak duration.** Default: 2 weeks. Reduce only if the parity test reveals zero material disagreements across the 5-symbol set.
- **What counts as a "material disagreement"?** Default: numeric fields differ by >5%, or a required field present in one and absent in the other. Anything below 5% is logged but not blocking.
- **Cache directory after deprecation.** Default: leave `cache/alphavantage/` on disk for one release after deletion (rollback safety). After that, the directory cleans itself out via TTL.
- **Telemetry on cutover day.** Add a one-off log line each time the morning sweep starts noting which provider is active. Lets the next maintainer pass diagnose post-mortem if something looks off.

## Exit criteria

**Cutover (immediate):**
- Default settings produce `fundamentals_source = "ibkr"`.
- Morning sweep over a 100-ticker test universe completes; logs show zero `Fetching ... from Alpha Vantage` entries on the fundamentals path. (AV news entries still expected.)
- Cross-phase tracer-bullet test passes: from a Claude Code session, `get_fundamentals(symbol="AAPL")` returns a `FundamentalData` whose populated fields match (within materiality threshold) the AV result for the same symbol the same day.
- Shadow comparison runs for 2 weeks; `QUESTIONS.md` has either an "all clean" entry or a list of materially-different symbols with field-level details.

**Deprecation (after soak):**
- `AlphaVantageFundamentalsProvider` is gone. `cargo build` doesn't reference `fetch_fundamental_data` anywhere on the fundamentals path (verified via grep).
- `cargo test` and pre-commit clean after deletion.
- Settings flag is either removed or its only valid value is `"ibkr"`.

## Gotchas

- **AV news still depends on `FinancialDataService`.** Don't accidentally delete the news-side AV code when removing the fundamentals-side. The news code lives in `services/financial_data_service/news.rs` and uses different functions.
- **TWS must be running for the morning sweep.** If the user's system doesn't have TWS up at 07:00, IBKR fundamentals fetches fail. Document in the project README + add a startup check that warns clearly. Long-term fix is the deferred Phase 9 daemon (prior roadmap).
- **Subscription lapse safety.** If the IBKR Reuters subscription lapses mid-cycle, every fetch fails. Recovery plan: temporarily set `fundamentals_source = "alpha_vantage"` (works while AV provider still exists pre-deletion). After deletion, recovery requires a code rollback. Document the trade-off; user accepts it.
- **Shadow-comparison cost.** Running both providers doubles fundamentals load. AV will rate-limit fast on a 100-ticker sweep — that's expected; the comparison code must tolerate AV errors and log them as "AV unavailable, no comparison" rather than failing the sweep.
- **Don't delete the AV cache eagerly.** Even after AV provider deletion, `cache/alphavantage/` may contain useful historical data for diff debugging. Leave untouched for at least one release after deletion.
- **Watch for "where's the data coming from" confusion.** After deletion, a developer reading `services/financial_data_service/` will see only news code and may not realize fundamentals moved. Top-of-file comment in `mod.rs` (Files section) prevents this.
