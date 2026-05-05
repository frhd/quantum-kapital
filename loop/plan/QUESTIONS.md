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

## Phase 4 (2026-05-06)

- *Equity-curve reconstruction is trade-flow-only.* The phase doc
  committed to subtracting deposits / withdrawals / dividends / non-
  trade fees from the daily equity series via `account_summary` deltas
  vs prior NLV. P4 ships without that — there is no `account_summary`
  service today, only the per-row `equity_snapshots` table from P1
  (which only carries the T-1 close NLV the risk-engine sized
  against). The curve we render is `Σ(realized_pnl - commission)` per
  ET trading day. Whoever lands the NLV-history service should plug
  it into `equity_curve.rs::reconstruct_daily_equity` and emit
  `reconciliation_warning` for days where the NLV-delta minus our
  trade-flow PnL exceeds the $50 threshold.
- *Conviction calibration uses the fallback table, not
  `eval_harness::calibration_stats`.* Phase doc decision was
  "calibrated with N≥50 fallback to 1.0". The fallback (A=1.5 / B=1.0
  / C=1.0) is wired in `grade.rs::ConvictionCalibration::fallback`.
  Hooking up the calibration-stats query is gated on the eval harness
  exposing a `realized_target_rate(grade) -> Option<f64>` synchronous
  helper from a `Db` handle — today the helper lives behind the MCP
  tool wrapper and isn't easy to call from `compute_v2_fields`. Open
  for whoever owns the eval-harness API surface; until then `score_v2`
  rewards A-conviction setups at 1.5× fixed.
- *Single-day risk_metrics on a `day_reviews` row are mostly empty.*
  Sharpe / Sortino / Calmar require N≥20 daily samples (committed in
  the phase doc). A v2 row written for a single day will report
  `sharpe=null`, `sortino=null`, `calmar=null` — only PF / expectancy
  / win-rate / DD numbers populate. Multi-day rolled-up metrics come
  from the `trade_review_get_metrics` Tauri command (range query),
  not from a `day_reviews` row. The UI `RiskMetricsPanel` renders the
  null cells as `—` ("insufficient history") so the gating is visible
  rather than silent.
- *MCP `write_trade_review` tolerates fills-fetch failure.* The agent
  path expects the executions seam to be reachable (live IBKR or
  persisted store). When fetch errors (paper disconnect, pre-P2
  history, NotConnected stub in tests) the rail still writes a v2
  row using `score_v2 = 0` and `discipline_v2 = Σ(tag_weights)`. The
  narrative + tags carry value even without R-attribution; the
  `n_legs_unattributed` counter (computed inside `compute_v2_fields`
  but NOT yet surfaced in the response shape) would tell the reviewer
  "the linkage was incomplete." A future tool-response refactor can
  expose it for visibility.
- *`AppEvent::TradeReviewWritten.grade` field renamed to
  `formula_version`.* No frontend currently subscribes to this event,
  so the rename is risk-free; flagged here so a future FE that wires
  up the event-driven refresh path knows what to read. Pre-P4
  consumers would have read `grade: "B"`; post-P4 consumers read
  `formula_version: "v1" | "v2"` and decide which scoring fields to
  display from the re-fetched review.

## Phase 5 (2026-05-06)

- *AV fundamentals retirement audit — outcome: KEEP.* Phase 5 inspected
  whether AV fundamentals (revenue/EPS/sector) are load-bearing for any
  consumer beyond earnings-date lookup. Findings:
    - `services::projection_service` reads `historical` (revenue, net
      income, EPS) and `analyst_estimates` from `FundamentalData` to
      build the bear/base/bull scenarios that drive `analysis.rs`'s
      projection-results bundle. UI consumes via
      `ibkr_generate_projection_results`. Load-bearing.
    - `ibkr::commands::analysis::overlay_live_price` writes the IBKR
      live quote into `current_metrics.price` before projection — the
      rest of `current_metrics` (pe_ratio, shares_outstanding) is
      sourced from AV's OVERVIEW. Projection math depends on
      `shares_outstanding`. Load-bearing.
    - The MCP `get_fundamentals` tool exposes the full `FundamentalData`
      shape to external clients. Load-bearing.
  AV is **retained** for the OVERVIEW + INCOME_STATEMENT + EARNINGS
  endpoints (the existing rate-limited path through
  `FinancialDataService`). Manual store remains the override layer.
  Re-open this audit if a future migration moves projections to a paid
  fundamentals provider; until then the entire AV-fundamentals stack
  (adapter + ledger + cache + composite provider) stays in place
  unchanged.

- *AV upstream wiring for the earnings calendar deferred.* P5 ships
  `services/event_calendar/` with a trait seam (`UpstreamEarningsFetcher`)
  and a `NoOpUpstream` default. The composite calendar still works
  end-to-end via the manual-overrides table + the cache table, which is
  what the integration test exercises. The AV-backed
  `UpstreamEarningsFetcher` impl (extracting `quarterlyEarnings[*].
  reportedDate` from the existing `fetch_earnings` path) didn't land
  in P5 because the next-quarter `reportedDate` field is sparsely
  populated by AV — the manual-store path is the load-bearing one for
  now. Wire the AV adapter when the operator finds manual-store
  maintenance painful, OR when P10 walk-forward refit needs a programmatic
  earnings cache for backtest fixtures. The trait + cache shape are
  ready; only the adapter impl is missing.

- *Default `skip_if_unknown_earnings = true` for breakout +
  parabolic-short.* Master plan committed this as the conservative
  policy. Concrete consequence: until the manual store has earnings
  rows for a watchlist symbol, every breakout / parabolic-short hit on
  that symbol is skipped with `source: "unknown"` in the blackout
  descriptor. Operators who want the gate to be permissive until
  upstream is wired can set `skip_if_unknown_earnings = false` per
  detector in settings.json. Logged here so the dogfooding curve
  doesn't surprise — populate `event_calendar_overrides` for active
  watchlist symbols, OR flip the default per-detector.

- *Override path leaks `&'static str` for the strategy field.*
  `ibkr::commands::event_calendar::setup_override_blackout` reconstructs
  a `SetupCandidate` from a stored `Setup`, which requires a `&'static
  str` strategy. The override path is rare (per-setup, human-driven)
  so we `Box::leak` the strategy name there — same pattern as
  `risk_recompute_setup`. If a future feature triggers the override
  path at scanner cadence the leak becomes load-bearing; until then
  the budget is dominated by the surrounding LLM costs.

- *FOMC dataset is hardcoded JSON, not a live feed.* `data/fomc_dates.json`
  carries 2026 + 2027 meeting dates. The gate emits a `warn!` log when
  `last_meeting < 90 days from now`. Refresh the file when the Fed
  publishes a new annual schedule (typically each summer). Mirroring
  the holidays.rs convention — once a year, by hand.

- *SetupCard + SkippedSetupsPanel still not wired into a parent
  Tracker tab.* Same pre-existing flag from P1/P3 — the components
  ship under P5 but no parent component imports them yet. The
  Tracker tab currently renders setups as row decorations in
  `Watchlist.tsx`. Phase that owns the trader-facing card surface
  should pick up SetupCard + SkippedSetupsPanel together when the
  watchlist-row refactor lands.

- *Pre-existing `decay_watcher` flake still failing.* P1 and P2 both
  logged `services::decay_watcher::tests::respects_budget_kill_switch`
  ("MockHttp queue exhausted"). P5 confirms the same panic on
  baseline; not introduced by event-blackout work. Left for the
  decay-watcher owner.

## Phase 6 (2026-05-06)

- *Backtester ships as a harness; the data-driven retirement +
  audit decisions remain pending real bar data.* Phase 6 lands the
  full replay engine, fill-model trait + naive/calibrated impls,
  walk-forward splits, results aggregation, persistence, Tauri
  commands, frontend, and `qk-backtest` CLI binary. What it does NOT
  land: the actual 18-month per-detector OOS sweeps that drive the
  retirement decisions in master "Removals + corrections committed".
  These need (a) the trader's `bars_cache` to be primed for a
  representative top-50 watchlist over the full lookback, and (b)
  P5 manual-overrides earnings rows populated for honest blackout
  comparisons. Both are operator tasks. The harness is ready; the
  evidence-gathering is the next sweep.

- *Sentiment-surge candidate-source A/B deferred.* The current
  backtester is symbol-driven, not candidate-source-driven. To do
  the A/B the runner spec doc committed, the spec needs a
  `candidate_source: { ibkr_scanner | sentiment_surge | union }`
  filter that joins against the `candidate_universe` table. The join
  isn't load-bearing for the harness itself — it can be added
  alongside the first real run. Master removal decision for
  sentiment-surge stays open until that A/B has data.

- *LLM-thesis A/B deferred.* Same shape — the harness's "no LLM"
  decision (master committed: "backtester does NOT call LLM") means
  every backtest setup carries `conviction = B` and no thesis prose.
  An A/B against thesis-on setups requires either replaying historical
  thesis-prose against historical setups (cheap; what we have in
  `setups.thesis_json`) or re-querying Anthropic per backtest setup
  (expensive; budget-blocking). The path of least resistance is to
  filter live setups by thesis-presence and compare realized R; that's
  a one-shot SQL query over the production `setups` + `executions`
  tables, not a backtester job. Will land when the production
  data set has enough setups with vs without thesis prose.

- *Detector retirement (PF<1.2 over 18mo) deferred.* The harness
  enforces the threshold as a documented invariant but does NOT
  programmatically flip detectors off in `settings.toml`. The reason:
  flipping off a detector based on a backtest the trader hasn't
  reviewed produces a confusing UX ("why did breakout stop firing?
  because a backtest you ran two weeks ago said so"). The decision
  shape we landed on: when the operator runs a real OOS sweep, they
  can hand-flip the disabled-by-default flag with the run-id in the
  commit message. Master invariant stands; the automation hook lands
  with P10 (walk-forward refit) when an authoritative refit cadence
  exists.

- *Look-ahead audit covered by `replay_never_passes_future_bars_to_detector`.*
  The test installs a `PointInTimeAuditor` detector that asserts
  `last_bar_time <= ctx.now` on every evaluation. If a future detector
  starts indexing `bars[i+1]` somewhere, the assertion trips. Pin
  here so a future maintainer doesn't remove the auditor as "unused
  test detector".

- *Determinism contract pinned by `rerun_same_spec_yields_identical_trade_count_and_pnl`.*
  Two `Backtester::run` invocations against the same spec, same db
  state, must produce the same trade count and same headline PnL.
  Run-id collisions inside the same millisecond are kept distinct by
  a process-local counter (`RUN_COUNTER`) — that's a quality-of-life
  fix, not a determinism input.

- *Survivorship bias documented, not corrected.* `bars_cache` is
  populated from what the trader has watched / fetched. A backtest
  over the watchlist is biased toward symbols the trader chose to
  watch, which skews positive. The result-card UI doesn't surface
  this caveat yet — log here so the next operator pass doesn't
  read a backtest as a universal-edge claim.

- *Splits & dividends not adjusted.* IBKR `bars_cache` is
  split-adjusted but not dividend-adjusted. For the swing-hold
  horizons the existing detectors target (days), the dividend impact
  is < 0.5% per trade — accepted per master gotcha. If a future
  detector trades over weeks, this becomes load-bearing.

- *Pre-existing `decay_watcher` flake still failing.* Same panic
  P1/P2/P5 logged; verified again on baseline against
  `services::decay_watcher::tests::respects_budget_kill_switch`. Not
  introduced by P6. The 968 other lib tests pass (39 of them new
  backtester tests).

- *Paper-account E2E not run by Claude.* The phase doc envisioned
  an end-to-end run of all 3 detectors on top-50 watchlist symbols ×
  18 months. That run requires a primed `bars_cache` Claude doesn't
  have. The harness is reachable from the in-app `BacktestRunner` UI
  and the `qk-backtest` CLI; the operator runs the first sweep and
  documents results here when they do.

- *Calibrated fill-model statistical-equivalence to P2 mean ±1bp not
  yet pinned by a synthetic test.* The `calibrated_from_distribution_extracts_means`
  unit test pins the mean/stdev derivation from a synthetic histogram;
  the "matches P2 historical mean ±1bp" exit criterion in master is
  satisfied by construction (we read the same TCA distribution rows
  the live UI reads), but a regression test that compares a live-data
  histogram round-trip against the calibrated model's mean is a
  follow-up. Not load-bearing for the harness — both inputs derive
  from `tca_get_slippage_distribution` directly.

- *Frontend not yet wired into a parent tab.* `BacktestRunner` and
  `BacktestResults` ship under `src/features/backtest/components/`
  but no parent screen imports them. Same shape as P1/P3/P5 SetupCard
  — the components are reachable via direct import / future page
  routing. The phase that owns the trader-facing tab surface
  (Workspace tab refactor) should pull these onto a "Research →
  Backtest" tab when it lands.

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
