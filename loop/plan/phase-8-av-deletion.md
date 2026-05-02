# Phase 8 — Full AV deletion (news cutover + module rip-out)

> Part of [Alpha Vantage → IBKR: Full Vendor Strip-out](master.md). See index for invariants.

**Status:** todo

**Depends on:** 5 (fundamentals already on IBKR + AV fundamentals adapter deleted), 7 (news provider trait shipped + IBKR news provider available)

**Goal:** Flip `news_source` default to `"ibkr"`. Run a 100-ticker morning sweep with both `fundamentals_source` and `news_source` set to `"ibkr"`. Confirm zero AV traffic. After ~2 weeks of clean operation under shadow news comparison, **delete every Alpha Vantage artifact from the repo**: `FinancialDataService` struct, `AlphaVantageNewsProvider`, `AlphaVantageRateLimiter` middleware, `cache/alphavantage/` directory references, `ALPHA_VANTAGE_API_KEY` env var (4 known sites), settings flags `fundamentals_source` + `news_source`, `.env.example` entries, the AV fundamentals fetchers (`overview.rs`, `income.rs`, `earnings.rs`) if Phase 5 left any. End-state: `cargo build` does not link anything Alpha-Vantage-shaped, and a fresh dev clone needs no AV credential to run.

## Files

**Cutover (immediate):**

- Touches: `src-tauri/src/config/settings.rs` — change default for `news_source` from `"alpha_vantage"` to `"ibkr"`. Existing settings files keep their stored value (no auto-migration).
- Touches: `src-tauri/src/lib.rs` — wire `IbkrNewsProvider` as the default branch in the news-provider construction match.
- New: `src-tauri/tests/integration/news_provider_parity.rs` — runs both providers against a 5-symbol fixture set, diff-asserts the field set (excluding sentiment scores). Live tests behind `#[ignore]` so CI doesn't need TWS.
- New: `src-tauri/src/services/news_provider/shadow.rs` — temporary shadow-comparison wrapper. Wraps the IBKR news provider, fires the AV provider in the background, logs coverage gaps. Active only when settings include `shadow_av_news_comparison: true`. Deleted at the end of the soak window.
- Touches: `loop/plan/QUESTIONS.md` — log shadow-comparison findings (symbols + windows where IBKR coverage is materially thinner than AV).

**Deletion (after ~2-week soak — separate commit):**

- Delete: `src-tauri/src/services/news_provider/alpha_vantage.rs`.
- Delete: `src-tauri/src/services/news_provider/shadow.rs`.
- Delete: `src-tauri/src/services/financial_data_service/` (entire directory — `mod.rs`, `news.rs`, `news_tests.rs`, plus any of `overview.rs`, `income.rs`, `earnings.rs` that survived Phase 5).
- Delete: `src-tauri/src/services/fundamentals_provider/alpha_vantage.rs` if Phase 5 chose to keep it as a fallback past its own soak.
- Delete: `src-tauri/src/middleware/alpha_vantage_rate_limit.rs` — Phase 1 created this; it has no callers after AV is gone.
- Delete: every `cache/alphavantage/*` reference. The directory itself is on-disk state, not source — leave it on the user's filesystem (TTL cleans it out) but remove anything in code that knows the path.
- Touches: `src-tauri/src/middleware/mod.rs` — drop `pub mod alpha_vantage_rate_limit;`.
- Touches: `src-tauri/src/services/mod.rs` — drop the `financial_data_service` module declaration.
- Touches: `src-tauri/src/config/settings.rs` — remove `alpha_vantage_api_key` field (line 319 today). Either delete `fundamentals_source` and `news_source` entirely or leave them as `String` constants set to `"ibkr"` for future flexibility — default: delete.
- Touches: `src-tauri/src/lib.rs` — drop the AV branch in both provider construction sites (line 116 today reads `ALPHA_VANTAGE_API_KEY`; the whole block goes). The `match settings.fundamentals_source.as_str()` and the equivalent for `news_source` collapse to a single direct construction of the IBKR provider.
- Touches: `src-tauri/src/services/news_interpreter/mod.rs` — remove any AV-shaped reads if Phase 7 left compatibility shims in place.
- Touches: `src-tauri/src/services/news_interpreter/tests.rs` — drop AV-flavored fixtures or rename them (they were just sample `NewsItem`s; structure should still be valid).
- Touches: `src-tauri/.env.example` — remove `ALPHA_VANTAGE_API_KEY` line.
- Touches: `CLAUDE.md` (root) — remove the "Alpha Vantage `ALPHA_VANTAGE_API_KEY`" reference under "Secrets". Update the falls-back-to-mock sentence (no longer true; behavior is now an explicit IBKR error).
- Touches: `src-tauri/CLAUDE.md` — same audit; remove AV-specific guidance.
- New: `CHANGELOG.md` entry (or commit message body if no CHANGELOG file exists) — explicit note that `ALPHA_VANTAGE_API_KEY` is no longer read; dev environments may safely unset it.

## Reuse

- Existing IBKR providers (Phases 4 + 7) — no changes to either.
- Existing `news_cache` SQLite table — backend unchanged, producer is now exclusively IBKR.
- `services/fundamentals_provider/test_support.rs::FakeFundamentalsProvider` and `services/news_provider/test_support.rs::FakeNewsProvider` — both stay; tests that need provider injection continue to use them.

## Decisions to make in this phase

- **Soak duration.** Default: 2 weeks for the news cutover, mirroring Phase 5. Reduce only if the parity test reveals zero coverage gaps across the 5-symbol set.
- **What counts as a "material coverage gap"?** Default: IBKR returns ≥80% of the items AV returned for the same `(symbol, window)`, judged by `time_published` overlap. Below 80% blocks deletion until the source mix is widened or the gap is explained.
- **`fundamentals_source` / `news_source` removal vs. retention.** Default: remove. They served the migration; carrying them forward as `String` constants is YAGNI.
- **CHANGELOG vs. commit-body announcement.** If the repo has no CHANGELOG today, ship the announcement in the deletion commit body and stop there. Don't introduce a CHANGELOG file just for this.
- **Cache directory cleanup.** Default: leave `cache/alphavantage/` on disk. The TTL on cached entries cleans them out passively; eager deletion risks losing data the user might want to diff against later.

## Exit criteria

**Cutover (immediate):**
- Default settings produce both `fundamentals_source = "ibkr"` and `news_source = "ibkr"`.
- Morning sweep over a 100-ticker test universe completes; logs show zero `Fetching ... from Alpha Vantage` entries on either path.
- Cross-phase tracer-bullet test passes: from a Claude Code session, the news-fetching MCP tool returns non-empty results for AAPL with all sources set to `"ibkr"`. `NewsInterpreter` produces a verdict from the IBKR-sourced cache row.
- Shadow comparison runs for ~2 weeks; `QUESTIONS.md` has either an "all clean" entry or a list of materially-thinner symbols with the source-mix decision.

**Deletion (after soak):**
- `cargo build` does not link `FinancialDataService`, `AlphaVantageRateLimiter`, `AlphaVantageNewsProvider`, or `AlphaVantageFundamentalsProvider`. Verified by absence in `cargo expand` or by deletion of the source files themselves.
- `rg -i "alpha.?vantage"` over `src-tauri/src` returns zero production hits (allowed: this plan, CHANGELOG entry, historical commit messages — none of which the grep crosses).
- `rg "ALPHA_VANTAGE_API_KEY"` over `src-tauri/`, `src/`, `.env.example`, root + nested `CLAUDE.md` files returns zero hits.
- A fresh dev clone with `ALPHA_VANTAGE_API_KEY` unset starts the app without any warning or error related to AV.
- All test suites still pass after deletion: `cargo test`, `pnpm test:run`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `pnpm lint`, `pnpm typecheck`. Pre-commit clean.
- The Phase 8 final-AV-elimination CI check from `master.md § Cross-phase verification` passes (greps for `alpha_vantage` / `AlphaVantage` / env var; build fails if any production reference survives).

## Gotchas

- **Don't delete `news_cache`.** It's the SQLite table backing news consumers; the producer changes, the storage doesn't. Touching the schema would force a migration for no benefit.
- **Don't delete the on-disk `cache/alphavantage/` directory.** Source code stops referencing it; the directory keeps existing on user filesystems with progressively-expiring data. Forcing a `rm -rf` would surprise users.
- **`NewsInterpreter` may have AV-shaped tests.** They reference `NewsItem` instances with sentiment fields populated; that's still a valid `NewsItem`, just a shape IBKR-sourced rows won't produce. Decide whether to rename / re-fixture or leave as-is.
- **`config/settings.rs` field removal is a settings-file migration risk.** Existing `~/.config/.../settings.json` files will have stale `fundamentals_source` / `news_source` fields; Serde defaults to ignoring unknown fields, so this should be a no-op — but verify with a manual round-trip.
- **`lib.rs` line 116 mentions Phase 10.** A comment in `lib.rs` says "Phase 10: shared FinancialDataService instance (the news interpreter shares this with the tracker pipeline)". When deleting, also delete or update that comment so the codebase doesn't carry a phantom phase reference.
- **`CLAUDE.md` is read by Claude on every session.** Forgetting to remove the AV mention there means future sessions will be told to look for something that no longer exists. Worth a final grep at the very end.
- **Shadow comparison cost.** Running both providers doubles news load. AV will rate-limit fast on a 100-ticker sweep — that's expected; the comparison code must tolerate AV errors and log them as "AV unavailable, no comparison" rather than failing the sweep.
- **TWS must be running.** After deletion, neither fundamentals nor news has any HTTP fallback. If TWS is down at sweep time, the run fails completely. Document loudly in the deletion commit body and update `CLAUDE.md`. Long-term answer is the deferred Phase 9 daemon (prior roadmap).
- **Read your own diff before committing the deletion.** The deletion commit will be large (file removals across many directories). `git diff --stat HEAD~1` after staging — confirm no production code outside the AV surface area got caught in the rm.
