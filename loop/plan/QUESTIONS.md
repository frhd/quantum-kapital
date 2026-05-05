# Cross-phase questions log

Append-only log. Issues raised during a phase that the phase intentionally did not fix — pre-existing flakes, scope-cut deferrals, decisions punted, retirement decisions, audit results. Each entry names the file/test/symbol so the next maintainer pass can find it.

Group entries under `## Phase N (YYYY-MM-DD)` headings. Don't backfill — write entries at the moment the issue is raised.

## Phase 1 (2026-05-05)

- *Pre-existing flake unrelated to risk-engine work.*
  `services::decay_watcher::tests::respects_budget_kill_switch` panics with
  "MockHttp queue exhausted" on baseline `main` (verified by stashing
  Phase 1 changes and re-running). Not introduced by P1; left as-is for
  P18-decay-watcher owner to fix.
- *SetupCard not yet wired into a tab.* The new
  `src/features/tracker/components/SetupCard.tsx` renders qty /
  dollar-risk / R-per-share / stale-equity warning, but no parent
  component imports it. The Tracker tab today shows setups as row
  decorations in `Watchlist.tsx`; pulling in SetupCard requires a
  watchlist-row refactor that's out of P1's scope. Phase that owns
  the trader-facing card surface should pick this up.
- *Conviction signal mapping — A=≥0.75, B=≥0.5, else C.* Locked here
  so future grade tuning has a documented baseline. The master plan
  decision was "sizing comes from LLM thesis", but P1 sources from
  the detector's `conviction_signal` field since thesis runs after
  insertion. Re-grading at thesis time is a future enhancement.
- *`risk_recompute_setup` recovers the conviction signal from the
  persisted grade*, mapping A→0.85, B→0.6, C→0.3. Recompute
  preserves grade across config-knob refreshes; the original
  detector signal is not stored on the row, so this is a one-way
  mapping. Acceptable for P1 since recompute is a niche path.

## Phase 2 (2026-05-05)

- *trade_legs schema item interpreted as struct fields, not a SQL
  table.* The phase doc lists "trade_legs adds strategy TEXT, setup_id
  INTEGER (nullable, NULL for pre-P2 legs)" under Migrations, but the
  service is in-memory (no `trade_legs` table exists today). P2 added
  the fields to the `TradeLeg` struct + carries them from
  `ExecutionRow` into the FIFO matcher's `OpenSlice`. If a future phase
  persists trade legs to SQLite the same column names land naturally.

- *Pre-existing `decay_watcher` flake still failing.* P1 logged this in
  the Phase 1 entry; P2 confirms the same `MockHttp queue exhausted`
  panic in `services::decay_watcher::tests::respects_budget_kill_switch`
  on baseline. Unrelated to TCA work; left for the decay-watcher owner.

- *Manual intent matching window pinned at 60 min.* The
  `tca_record_manual_intent` command always uses the limit-side window
  even when the trader is recording for an out-of-band MARKET fill. In
  practice manual intents are recorded after the fill arrives, so the
  window only needs to cover "I confirmed and clicked record within
  60m" — tighter values would make the path noisier without buying
  attribution accuracy. Revisit if dogfooding shows otherwise.

- *Out-of-band fill match is best-effort: the matcher doesn't
  back-link a previously-stored unattached fill when a manual intent
  arrives later.* The next ingestor tick (or any future
  `attach_fills_for_account_today` call) will pick it up because
  `attach_fills_for_account_date` queries all of the day's fills. So
  the linkage eventually arrives without an explicit retry path —
  documented here so a future maintainer doesn't add one.

## Phase 5 (TBD)

- *Reserved for AV fundamentals retirement audit result.* P5 inspects whether AV fundamentals (revenue/EPS/sector) are load-bearing for any consumer beyond earnings-date lookup. If not, AV fundamentals fallback retires in this phase's diff.

## Phase 6 (TBD)

Reserved entries (filled when phase runs):

- *Per-detector OOS profit-factor over 18 months.* If any detector falls below the 1.2 threshold committed in master, document it here, list the diff that disabled it, and link to the backtest run id.
- *Sentiment-surge candidate-source A/B.* Realized R from sentiment-surge-sourced candidates vs IBKR-scanner-sourced candidates. Drives master removal decision.
- *LLM-thesis A/B.* Outcome lift comparison; drives demote-to-optional decision.

## Phase 7 (TBD)

- *Vol-adjusted exit shadow result.* After 4-week shadow, document OOS Sharpe / profit-factor for `atr_scaled` vs `static_2r_3r`. Cutover decision recorded here.

## Phase 9 (TBD)

- *Per-detector regime preference justifications.* For each detector, the on-regime vs all-regime backtest comparison that justifies the declared `preferred_regimes`. Linked to backtest run ids.

## Phase 10 (TBD)

- *Settings.toml semantics migration.* When refit lands, settings.toml ceases to be active params and becomes bounds. Document the one-shot migration here.

## Phase 11 (TBD)

- *First tilt episode in production.* When the first real tilt fires, capture the trigger details, override behavior, and trader feedback here. Calibrates whether thresholds are tuned right.

## Cross-phase open

- *Override frequency monitoring.* Every gate (blackout / concentration / regime / tilt) supports per-setup override. If any single gate has > 30% override rate over 60 days of live use, the gate is too strict OR the trader is rationalizing — review here.
