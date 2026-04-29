# Tracker Implementation Plan

Test-driven, incremental implementation of the strategy-driven watchlist with continuous LLM reasoning for Quantum Kapital.

## What this plan delivers

A **Tracker** subsystem that persists a watchlist of tickers (sourced from the scanner, manual entry, or news), evaluates them periodically against pluggable strategy detectors (Breakout / Episodic Pivot / Parabolic Short), layers Claude reasoning on top (per-candidate thesis, decay-watcher, news interpreter, daily ranker), and surfaces alerts + a ranked daily candidate list. Surveillance only — no order placement.

Locked-in profile: **disciplined swing** (0.5–1% risk/trade, 5–7 concurrent setups, daily setups with intraday triggers, 2R/3R targets), **regime-agnostic** detector weighting, **tiered cadence** (daily EOD + intraday-5m for in-play tickers).

## How to use this plan

Every phase follows the same TDD discipline:

1. Read the phase file in `impl/`.
2. **Write the tests listed under "Test plan" first.** Run `cargo test` (or `pnpm test:e2e` for UI) and confirm they fail (red).
3. Implement under "Implementation tasks" until the tests pass (green).
4. Refactor and run `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and `cargo fmt --manifest-path src-tauri/Cargo.toml`.
5. Tick the boxes inline as you go. Commit at phase boundaries.
6. If a phase produces information later phases need but code can't capture (calibration thresholds, prompt revisions, backtest hit-rates), write it to the relevant scratchpad in `impl/scratch/`.

**Do not start a phase whose dependencies are unchecked.** The dependency graph is in each phase's "Depends on" section.

## References

- **Design / strategy doc:** `~/.claude/plans/ultrathink-in-the-context-toasty-spindle.md` (the architectural rationale and phase summary)
- **Project conventions:** `CLAUDE.md` (build commands, layered backend pattern, pre-commit hooks, TDD with `MockIbkrClient`)
- **Pre-commit:** `.pre-commit-config.yaml` (cargo fmt + clippy run on every commit; never use `--no-verify`)
- **IBKR API surface:** `IBKR_API_INTERFACES.md`
- **Alpha Vantage setup:** `ALPHA_VANTAGE_SETUP.md`, `FUNDAMENTAL_DATA_API.md`
- **Settings:** `SETTINGS_GUIDE.md`

No `CONTRIBUTING.md` exists at the time of writing; use `CLAUDE.md` as the canonical contributor guide.

## Cross-phase scratchpads

These files persist information that lives between phases but isn't expressed in code. Phases reference them where appropriate.

- `impl/scratch/schema-decisions.md` — schema evolution log, migration rationale, indexes added or removed.
- `impl/scratch/detector-calibration.md` — threshold choices for each detector (volume multiples, ATR cutoffs, gap sizes), why they were picked, observations from running on real data.
- `impl/scratch/llm-prompts.md` — prompt versions for thesis / decay / news / ranker, model choice rationale, observed token counts and quality issues, cache-hit rates.
- `impl/scratch/backtest-results.md` — hit-rate stats per detector + LLM-assisted vs unassisted comparisons.

## Architecture changes

These are the structural additions to the existing layered architecture (`config / events / ibkr{client,commands,types,state} / middleware / services / utils`):

1. **New top-level module: `storage/`** at `src-tauri/src/storage/` — owns SQLite via `rusqlite` + `r2d2` connection pool, migrations runner, embedded `schema.sql`. Exposed to services through an `Arc<Db>`. The existing `services/cache_service.rs` (file-based JSON, 7-day TTL) is **kept as-is** for fundamentals/projections. Bars, news, tracker rows, setups, alerts, and LLM call ledger live in SQLite.

2. **New top-level module: `strategies/`** at `src-tauri/src/strategies/` — `StrategyDetector` trait, `MarketContext` data envelope, `SetupCandidate` output, plus one file per detector. Detectors are registered into a `DetectorRegistry` constructed at startup so adding a new strategy = one new file + one register call.

3. **`IbkrState` gains four fields:** `db: Arc<Db>`, `tracker: Arc<TrackerService>`, `historical_data: Arc<HistoricalDataService>`, plus phase-3 `eod_handle` / `intraday_handle: Arc<RwLock<Option<StreamHandle>>>` mirroring the existing `daily_pnl_handle` / `scanner_handle` pattern in `state.rs:41-42, 92-110`.

4. **`AppEvent` gains:** `SetupDetected`, `SetupInvalidated`, `TickerStatusChanged`, `MorningPackReady`. Emitted via the existing `EventEmitter`.

5. **`AppConfig.api`** gains `anthropic_api_key: Option<String>` and `daily_llm_budget_usd: f64`. **`AppConfig`** gains a new `detectors` block (Phase 22) for tunable thresholds.

6. **New frontend feature directory `src/features/tracker/`** with Watchlist, AddToTrackerDialog, AlertFeed, MorningPack components and `useWatchlist` / `useTrackerEvents` hooks.

7. **One scanner-row enhancement:** `ScannerResults.tsx` gains an "Add to tracker" action alongside the existing analysis deep-link. No other existing file's responsibility shifts.

No existing public API or stored format is broken or migrated. The plan is purely additive.

## Phases

Each phase is an independently shippable, test-covered slice. The numbering reflects dependency order; some phases (3, 11, 22) can move earlier if convenient.

### Foundation (1–5)

- [x] **Phase 01** — SQLite foundation (Db, migrations, full schema upfront) — `impl/phase-01-sqlite-foundation.md`
- [x] **Phase 02** — Historical bars service (`IbkrClient::historical_data` + cache) — `impl/phase-02-historical-bars.md`
- [ ] **Phase 03** — Alpha Vantage news service + cache — `impl/phase-03-news-service.md`
- [ ] **Phase 04** — Tracker persistence (types, service, Tauri commands) — `impl/phase-04-tracker-persistence.md`
- [ ] **Phase 05** — Tracker UI + scanner "Add to tracker" integration — `impl/phase-05-tracker-frontend.md`

### Detector framework (6–10)

- [ ] **Phase 06** — `StrategyDetector` trait, `MarketContext`, `SetupCandidate`, registry — `impl/phase-06-detector-framework.md`
- [ ] **Phase 07** — Breakout detector + table-driven tests — `impl/phase-07-breakout-detector.md`
- [ ] **Phase 08** — Episodic Pivot detector + tests — `impl/phase-08-episodic-pivot-detector.md`
- [ ] **Phase 09** — Parabolic Short detector + tests — `impl/phase-09-parabolic-short-detector.md`
- [ ] **Phase 10** — `tracker_run_now` command + setup persistence — `impl/phase-10-run-now.md`

### Scheduling (11–15)

- [ ] **Phase 11** — Market calendar utility (RTH + holidays) — `impl/phase-11-market-calendar.md`
- [ ] **Phase 12** — Tracker status state machine + transitions — `impl/phase-12-status-state-machine.md`
- [ ] **Phase 13** — EOD scheduler (16:05 ET full sweep) — `impl/phase-13-eod-scheduler.md`
- [ ] **Phase 14** — Intraday scheduler (5-min RTH for in-play) — `impl/phase-14-intraday-scheduler.md`
- [ ] **Phase 15** — Setup events plumbing + frontend listeners — `impl/phase-15-setup-events.md`

### LLM reasoning layer (16–20)

- [ ] **Phase 16** — Anthropic client foundation + budget kill-switch — `impl/phase-16-anthropic-client.md`
- [ ] **Phase 17** — Thesis prompt (Sonnet 4.6, tool-use forced JSON) — `impl/phase-17-thesis-prompt.md`
- [ ] **Phase 18** — Decay-watcher prompt (Haiku 4.5, intraday) — `impl/phase-18-decay-watcher.md`
- [ ] **Phase 19** — News interpreter prompt (Haiku 4.5) — `impl/phase-19-news-interpreter.md`
- [ ] **Phase 20** — Daily ranker (Sonnet 4.6) + MorningPack UI — `impl/phase-20-daily-ranker.md`

### Polish (21–23)

- [ ] **Phase 21** — AlertFeed UI (rolling alerts, mark-as-seen) — `impl/phase-21-alert-feed-ui.md`
- [ ] **Phase 22** — Configurable detector parameters in settings — `impl/phase-22-detector-config.md`
- [ ] **Phase 23** — Backtest replay mode — `impl/phase-23-backtest.md`

## Out of scope

- Order placement / live trading. `ibkr_place_order` exists but is not wired into the tracker.
- Options strategies. Detectors operate on equities only.
- Multi-account aggregation.
- Portfolio-aware position sizing (the system suggests stops/targets relative to setup R, but does not know account size — that intentionally stays the user's responsibility).
- Web UI / cloud sync — desktop-only, single-user.
- Pre-market / after-hours data beyond what IBKR delivers as RTH-extended bars.

## Risks tracked across phases

| Risk | First mitigated in | Notes |
|---|---|---|
| IBKR historical-data rate limit | Phase 02 | Reuse `RateLimiter`; SQLite cache eliminates repeats. |
| Alpha Vantage 25 calls/day free tier | Phase 03 | News refresh capped; document upgrade path. |
| LLM cost overrun | Phase 16 | Daily budget kill-switch + prompt caching + Haiku for hot paths. |
| LLM hallucination | Phase 17 | Detectors emit structured `raw_signals`; prompts reason over numbers, not narratives. |
| Setup-active rows accumulating | Phase 12 | `cool_down` TTL + nightly cleanup in EOD job. |
| News-driven false positives for EP | Phase 08, refined Phase 19 | Sentiment polarity must align with gap direction; LLM news interpreter as second filter. |
