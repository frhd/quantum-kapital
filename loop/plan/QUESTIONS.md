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

## Phase 3 (2026-05-06)

- *Outside-RTH deferred-ticket queue not implemented.* Master phase
  doc committed: "brackets only submit during RTH; pre-RTH 'Take
  Setup' queues a deferred ticket that fires at the open or expires
  at 09:35 ET if conditions changed." P3 ships the simpler half —
  the bracket placer routes through `IbkrClient::place_bracket` only
  when called, with no RTH gate or queueing scheduler. In practice
  the trader sees an IBKR rejection if they hit Send pre-RTH on a
  symbol where RTH-only orders are required. A dedicated deferred
  queue (storage-backed table + open-time scheduler + condition
  re-check) is its own subsystem and was de-scoped to keep P3 inside
  one calendar week. Re-open when pre-market trading is a
  load-bearing flow; until then the manual "wait for the open" path
  is the workaround.

- *Paper-account E2E not run by Claude.* The phase doc requires:
  "Paper-account E2E (manual run, documented): take a real setup on
  IBKR paper, observe parent + 3 children visible in TWS as one OCA
  group, fill leg by leg, observe stop qty reducing." Claude can't
  drive a paper-account session — the maintainer who first runs the
  TakeSetupModal against their paper account should append the
  observed behaviour here (parent/stop/3 targets visible, fills
  reducing each other, OCA semantics on partial fills). Until that
  happens the exit criterion is satisfied by the unit-test bracket
  simulation + the tracer-bullet test, not by a live IBKR
  observation.

- *SetupCard still not wired into a user-facing screen.* P1 already
  flagged this; P3 added the "Take Setup" button to `SetupCard.tsx`
  and the `TakeSetupModal` portal, but no parent component imports
  `SetupCard` yet (only `RankedSetupCard.tsx` references it without
  rendering it). The modal is reachable via tests and via direct
  import; the user-facing tracker tab will pick it up when the
  watchlist-row refactor lands. Phase that owns the tracker UI
  surface should pull SetupCard onto the visible Tracker tab.

- *Static 50/30/20 ladder hardcoded — runner pinned at 3R.* P3 ships
  the ladder as a const (`STATIC_TARGET_LADDER_PCT` /
  `STATIC_TARGET_R_MULTIPLES`). Master decision committed: "ship
  with 50/30/20 fixed; ATR-trail logic is P7." When P7 lands, the
  runner stops pinning at 3R and starts trailing on ATR — the
  `bracket_groups.targets_json` column carries the spec at submit
  time, so older rows stay readable under the static ladder while
  new rows pick up ATR-driven prices.

- *No fill-status reconciler yet.* `bracket_groups.last_status`
  defaults to `open` and only flips to `canceled` when the trader
  hits Cancel via `order_ticket_cancel_bracket`. A future reconciler
  will subscribe to IBKR `orderStatus` events and flip the row to
  `partial` / `filled` / `stopped` based on the OCA-group fills.
  Phase that owns the post-fill stream picks this up; P4 grading is
  resilient to stale status rows because attribution reads through
  `executions.setup_id` (always populated by P2 attach), not through
  `bracket_groups.last_status`.

- *Override-stop reason not separately persisted.* The modal accepts
  an override stop and a free-text reason, but `bracket_groups`
  stores the reason in `qty_override_reason`, shared between qty and
  stop overrides. Master plan only mandated `qty_override_reason`;
  since the column is free-text and the trader's note can describe
  whichever override they used, we kept the schema minimal. If
  per-channel override tracking is needed (qty vs stop independently),
  add a sibling column.

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
