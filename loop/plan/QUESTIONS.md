# Cross-phase questions log

Append-only log. Issues raised during a phase that the phase intentionally did not fix — pre-existing flakes, scope-cut deferrals, decisions punted, retirement decisions, audit results. Each entry names the file/test/symbol so the next maintainer pass can find it.

Group entries under `## Phase N (YYYY-MM-DD)` headings. Don't backfill — write entries at the moment the issue is raised.

## Phase 1 (2026-05-05)

- *Pre-existing flake unrelated to risk-engine work.*
  `services::decay_watcher::tests::respects_budget_kill_switch` panics with
  "MockHttp queue exhausted" on baseline `main` (verified by stashing
  Phase 1 changes and re-running). Not introduced by P1; left as-is for
  P18-decay-watcher owner to fix.

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
