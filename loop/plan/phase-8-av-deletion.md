# Phase 8 — AV news deletion (fundamentals AV adapter retained as fallback)

> Part of [Alpha Vantage strip-out: manual MCP fundamentals + IBKR news](master.md). See index for invariants.

**Status:** todo

**Depends on:** 5 (composite fundamentals path is the production default + AV guardrails active), 7 (news provider trait shipped + IBKR news provider available)

**Goal:** Flip `news_source` default to `"ibkr"`. Run a 100-ticker morning sweep with the IBKR news provider. Confirm zero AV news traffic. After ~2 weeks of clean operation under shadow news comparison, **delete every Alpha Vantage news artifact**: `AlphaVantageNewsProvider`, the AV news fetcher in `services/financial_data_service/news.rs`, the news-side `news_source` settings flag, the news-side `ALPHA_VANTAGE_API_KEY` references. End-state: `cargo build` does not link `AlphaVantageNewsProvider`; the news pipeline is exclusively IBKR-sourced.

**Important scope clarification (versus the previous plan revision):** This phase deletes **only** the AV news code. The AV fundamentals adapter (`services/financial_data_service/{mod.rs, overview.rs, income.rs, earnings.rs}`), the `AlphaVantageRateLimiter` middleware (Phase 1), the `cache/alphavantage/` directory references, and the `ALPHA_VANTAGE_API_KEY` env var **all survive** as part of the opportunistic fundamentals fallback path established in Phases 4 + 5. Hard Invariant #9 codifies this. The original "delete every Alpha Vantage artifact" goal is partially deferred: news artifacts go now; fundamentals artifacts stay until/unless the manual MCP store achieves coverage so high that the AV fallback is never reached for ~3 months — which would warrant a separate deletion phase, out of scope here.

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
- Delete: `src-tauri/src/services/financial_data_service/news.rs` and its sibling `news_tests.rs` if present.
- Touches: `src-tauri/src/services/financial_data_service/mod.rs` — remove `pub mod news;` declaration; the AV fundamentals fetchers (`overview.rs`, `income.rs`, `earnings.rs`) **stay**. Add a top-of-file comment: `// AV fundamentals adapter retained as opportunistic fallback (see CompositeFundamentalsProvider). News path is fully migrated to IBKR — see services/news_provider/.`
- Touches: `src-tauri/src/services/financial_data_service/mod.rs` — remove the `fetch_news_sentiment` method that lived alongside `fetch_fundamental_data`; keep only the fundamentals path.
- Touches: `src-tauri/src/lib.rs` — drop the `news_source = "alpha_vantage"` branch in the provider construction match. The construction collapses to a direct `IbkrNewsProvider` instantiation. The `ALPHA_VANTAGE_API_KEY` read at line ~116 (today) is **kept** because the surviving AV fundamentals adapter still needs it.
- Touches: `src-tauri/src/services/news_interpreter/mod.rs` — remove any AV-shaped reads if Phase 7 left compatibility shims in place.
- Touches: `src-tauri/src/services/news_interpreter/tests.rs` — drop AV-flavored news fixtures or rename them (they were just sample `NewsItem`s; structure is still valid, just decouples test naming from a vendor we no longer use).
- Touches: `src-tauri/.env.example` — keep `ALPHA_VANTAGE_API_KEY` with a comment noting it's used only by the fundamentals fallback (was the news+fundamentals key; now narrower).
- Touches: `src-tauri/src/config/settings.rs` — delete `news_source` field. Keep `alpha_vantage_api_key` field (line 319 today) — fundamentals fallback still needs it.
- Touches: `CLAUDE.md` (root) — update the "Secrets" section to clarify that `ALPHA_VANTAGE_API_KEY` is now used only for the **fundamentals fallback** (manual MCP `set_fundamentals` is the primary path). Drop the news mention.
- Touches: `src-tauri/CLAUDE.md` — same audit; remove AV news guidance, retain AV fundamentals fallback documentation.
- New: commit message body (no separate CHANGELOG file in this repo) — explicit note that `news_source` setting is removed; existing settings.json files with that field will silently ignore it (Serde `unknown_field` handling).

## Reuse

- Existing IBKR news provider (Phase 7) — no changes.
- Existing `news_cache` SQLite table — backend unchanged, producer is now exclusively IBKR.
- `services/fundamentals_provider/test_support.rs::FakeFundamentalsProvider` and `services/news_provider/test_support.rs::FakeNewsProvider` — both stay; tests that need provider injection continue to use them.
- AV fundamentals adapter (`services/financial_data_service/{mod.rs, overview.rs, income.rs, earnings.rs}`) — retained, no changes here. Continues to serve `CompositeFundamentalsProvider`.
- AV rate limiter middleware (`middleware/alpha_vantage_rate_limit.rs`) — retained for the surviving AV fundamentals path.

## Decisions to make in this phase

- **Soak duration.** Default: 2 weeks for the news cutover. Reduce only if the parity test reveals zero coverage gaps across the 5-symbol set.
- **What counts as a "material coverage gap"?** Default: IBKR returns ≥80% of the items AV returned for the same `(symbol, window)`, judged by `time_published` overlap. Below 80% blocks deletion until the source mix is widened or the gap is explained.
- **`news_source` removal vs. retention.** Default: remove. It served the migration; carrying it forward as a `String` constant is YAGNI. The fundamentals-side has no equivalent flag (composite is unconditional).
- **Cache directory cleanup for news.** AV news cached to `news_cache` (SQLite) — that table stays because IBKR writes to it now. There is no on-disk AV news cache directory to clean.
- **`ALPHA_VANTAGE_API_KEY` warning when unset.** Currently Phase 3 made unset key surface as a typed error in the analysis UI. Decide whether the Phase 8 deletion commit also adds a startup warning. Default: no — the composite gracefully serves manual-store-only when the key is unset; surfacing an error only on the analysis screen is the right level.

## Exit criteria

**Cutover (immediate):**
- Default settings produce `news_source = "ibkr"`.
- Morning sweep over a 100-ticker test universe completes; logs show zero `Fetching ... from Alpha Vantage` entries on the news path. (AV fundamentals fetches still possible if a symbol is opened that's not in the manual store, but those are user-driven not sweep-driven.)
- Cross-phase tracer-bullet test (`master.md § Cross-phase verification #2`) passes: from a Claude Code session, the news-fetching MCP tool returns non-empty results for AAPL with `news_source = "ibkr"`. `NewsInterpreter` produces a verdict from the IBKR-sourced cache row.
- Shadow comparison runs for ~2 weeks; `QUESTIONS.md` has either an "all clean" entry or a list of materially-thinner symbols with the source-mix decision.

**Deletion (after soak):**
- `cargo build` does not link `AlphaVantageNewsProvider` or any code in `services/financial_data_service/news.rs`. Verified by absence in `cargo expand` or by deletion of the source files themselves.
- `rg -n 'fetch_news_sentiment|AlphaVantageNews|news_source' src-tauri/src` over production code returns zero hits.
- AV fundamentals adapter still compiles + links: `cargo build` includes `services/financial_data_service/mod.rs`, `overview.rs`, `income.rs`, `earnings.rs`. Composite provider's e2e test (Phase 5) still passes.
- A fresh dev clone with `ALPHA_VANTAGE_API_KEY` unset starts the app without panic; opening the analysis screen for a symbol not in the manual store surfaces the typed `Other("Alpha Vantage API key not configured")` error in the UI; manual store reads still work.
- All test suites still pass after deletion: `cargo test`, `pnpm test:run`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `pnpm lint`, `pnpm typecheck`. Pre-commit clean.
- The Phase 8 final-AV-news-elimination CI check from `master.md § Cross-phase verification` passes (greps for `AlphaVantageNews`, `news_source`, `fetch_news_sentiment`; build fails if any production reference survives).

## Gotchas

- **Don't delete `news_cache` SQLite table.** It's the storage backing news consumers; the producer changes, the storage doesn't. Touching the schema would force a migration for no benefit.
- **Don't delete `services/financial_data_service/mod.rs` or its fundamentals fetchers.** Hard Invariant #9: the AV fundamentals adapter survives Phase 8. Deleting its module would break `CompositeFundamentalsProvider`.
- **Don't delete `middleware/alpha_vantage_rate_limit.rs`.** Same reason — used by the surviving AV fundamentals path.
- **Don't delete `cache/alphavantage/` directory.** Source code keeps referencing it for the fundamentals path; the directory holds AV fundamentals cache files that the composite provider's stale-fallback reads.
- **Don't delete the `alpha_vantage_api_key` field from `settings.rs`.** The fundamentals adapter still reads it.
- **`NewsInterpreter` may have AV-shaped tests.** They reference `NewsItem` instances with sentiment fields populated; that's still a valid `NewsItem`, just a shape IBKR-sourced rows won't produce. Decide whether to rename / re-fixture or leave as-is.
- **`config/settings.rs` field removal is a settings-file migration risk.** Existing `~/.config/.../settings.json` files will have stale `news_source` field; Serde defaults to ignoring unknown fields, so this should be a no-op — but verify with a manual round-trip.
- **`lib.rs` may have a "Phase 10" comment.** A historical comment in `lib.rs` references "Phase 10: shared FinancialDataService instance (the news interpreter shares this with the tracker pipeline)". When deleting the news-side wiring, also delete or update that comment so the codebase doesn't carry a phantom phase reference.
- **`CLAUDE.md` is read by Claude on every session.** Forgetting to update the AV mention there means future sessions will be told to look for things that no longer exist (AV news) or misframe what survives (AV fundamentals fallback). Final grep at the very end.
- **Shadow comparison cost.** Running both news providers doubles news load. AV will rate-limit fast on a 100-ticker sweep — that's expected; the comparison code must tolerate AV errors and log them as "AV unavailable, no comparison" rather than failing the sweep.
- **TWS must be running for news.** After deletion, news has no HTTP fallback. If TWS is down at sweep time, the news pass fails completely. Document loudly in the deletion commit body and update `CLAUDE.md`. Long-term answer is the deferred Phase 9 daemon (prior roadmap).
- **Read your own diff before committing the deletion.** The deletion commit will be moderate (a handful of file removals). `git diff --stat HEAD~1` after staging — confirm no surviving AV code (fundamentals adapter, rate limiter, env var sites) got caught in the rm.
