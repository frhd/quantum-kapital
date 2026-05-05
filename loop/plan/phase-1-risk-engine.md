# Phase 1 — Risk Engine: position sizing + R primitives

> Part of [Quantum Kapital → Quant-Decisions-In-Code](master.md). See index for invariants.

**Status:** in-progress (started 2026-05-05)

**Depends on:** none (foundation phase)

**Goal:** Every detected setup leaves Phase 1 with a deterministic share quantity, dollar-risk, and per-share R. The trader stops sizing in their head. This is the foundation every later phase reads from — without per-trade R, attribution, grading, backtesting, and tilt are all guesswork.

## Files

- New: `src-tauri/src/services/risk_engine/mod.rs` — `RiskEngine` service: `size(&SetupCandidate, &EquitySnapshot, &RiskConfig) -> Sizing`
- New: `src-tauri/src/services/risk_engine/equity_snapshot.rs` — Pulls T-1 close NLV from IBKR account summary; cached daily; fallback policy on stale.
- New: `src-tauri/src/services/risk_engine/sizing.rs` — Conviction-scaled risk math. Pure function, fully unit-tested with reference cases.
- New: `src-tauri/src/services/risk_engine/types.rs` — `Sizing { qty, dollar_risk, r_per_share, conviction_multiplier, equity_at_decision, version }`, `EquitySnapshot { nlv, as_of, source }`, `RiskConfig`.
- Touches: `src-tauri/src/strategies/candidate.rs` — Add `sizing: Option<Sizing>` field on `SetupCandidate` (None for pre-P1 rows; written by `TrackerRunner` after `size()` returns).
- Touches: `src-tauri/src/services/tracker_runner/mod.rs` — Call `RiskEngine::size` after thesis generation, before persisting setup.
- Touches: `src-tauri/src/storage/migrations/` — New migration: `setups` adds `qty`, `dollar_risk_cents`, `r_per_share_cents`, `equity_at_decision_cents`, `sizing_version`. (Cents to keep integer.)
- Touches: `src-tauri/src/lib.rs` — Construct `RiskEngine`, `app.manage` it.
- Touches: `src/features/tracker/components/SetupCard.tsx` — Surface qty / dollar-risk / R-per-share. Show "ungated equity" warning if snapshot is stale > 1 trading day.
- New: `src/shared/api/riskEngine.ts` — Tauri command wrappers (`risk_get_config`, `risk_set_config`, `risk_recompute_setup`).
- New: `src-tauri/src/ibkr/commands/risk.rs` — Tauri command handlers wrapping `RiskEngine`.

## Tools / endpoints exposed

| Tauri command | Purpose |
|---|---|
| `risk_get_config` | Read current `RiskConfig` (risk_pct per conviction, max position pct, equity-snapshot policy). |
| `risk_set_config` | Persist new `RiskConfig` to settings. |
| `risk_recompute_setup` | Rerun `RiskEngine::size` for a stored setup (used after equity snapshot refresh). |

## Reuse (no new business logic this phase outside `risk_engine/`)

- `IbkrClient::get_account_summary` for NLV. Wrap behind a `EquityFetcher` trait; mock in tests.
- `SetupCandidate::trigger_price` and `stop_price` are already populated by detectors — `size()` consumes them.
- `MockIbkrClient` in `ibkr/mocks.rs` for unit tests.
- `Db` SQLite handle for `equity_snapshots` table and config persistence.
- Existing `EventEmitter` to publish `SetupSized { setup_id, sizing }`.

## Decisions to make in this phase

- **Equity snapshot cadence.** Default committed: T-1 close NLV, refreshed at next open. Decide: do we refresh on cash deposits / withdrawals same-session, or wait? **Decision: wait.** Avoids whiplash sizing. User-triggered "force refresh" command is OK.
- **Conviction multipliers.** Defaults committed (A=1.0×, B=0.66×, C=0.33×). Decide: cap at 1.0× until P4 calibration validates, OR allow 1.5× for A immediately. **Decision: cap at 1.0× until P4.** Conviction multiplier > 1.0 not exposed in `RiskConfig` schema this phase.
- **Round-lot policy.** Decide: round qty to nearest share, nearest 5, or trader-configurable. **Decision: nearest share, integer qty floor.** Re-evaluate after P3 confirms IBKR partial-fill behavior on tiny qty.
- **Minimum dollar-risk floor.** Decide: skip setups whose computed dollar-risk < $10 (commission noise dominates) OR ship anyway. **Decision: skip with `sizing_skipped: "below_min_risk"` reason persisted.** Floor configurable; default $10.
- **Per-conviction max-position-pct cap.** Decide: cap notional position at X% of equity even if dollar-risk math says otherwise (low-vol stocks blow this up). **Decision: cap at 25% of equity per position; configurable.** Records `cap_applied: true` when cap binds.

## Exit criteria

- `cargo test risk_engine::` passes ≥ 12 reference cases covering: A/B/C conviction, stop-distance variations, equity-cap binding, min-dollar-risk floor, snapshot-stale fallback.
- Integration test: `tests/risk_engine_e2e.rs` walks a fixture setup through `TrackerRunner` end-to-end against `MockIbkrClient`; verifies `setups.qty` and `setups.dollar_risk_cents` are persisted and `SetupSized` emits.
- Frontend: setup card displays qty, dollar-risk, R-per-share. Stale-snapshot banner appears when snapshot age > 1 trading day. Verified via Playwright headless against `pnpm dev:browser`.
- Migration applied cleanly on a copy of the live SQLite (`tracker.sqlite` from app data dir); pre-existing setups have NULL sizing fields and read fine.
- `risk_get_config` / `risk_set_config` round-trip the full config struct; default config matches `Defaults committed` table in master.
- CI invariant: `clippy -D warnings` passes; no new file > 300 Rust LOC / 200 TS LOC.

## Gotchas

- **Equity snapshot must come from `account_summary`'s NetLiquidation, NOT cash balance.** Position MTM is part of risk capital. Test against an account with open positions in the fixture.
- **Conviction is from LLM thesis, which may be absent.** When `setup.thesis.conviction` is `None`, default to `C` and log a warning — don't crash, don't size at A.
- **Stop-price can equal trigger-price** in bizarre detector outputs (defensive code in detectors should prevent this, but `RiskEngine` must reject `r_per_share == 0` with `sizing_skipped: "zero_r"`, not divide by zero).
- **Settings hot-reload.** `RiskConfig` lives in `SettingsState`; changing risk_pct mid-session must NOT retroactively resize already-persisted setups. Add `sizing_version` so a future "resize" tool can replay deterministically.
- **Equity-snapshot cache invalidation across processes.** The MCP server bridge (`bin/mcp-server.rs`) is a separate process; it doesn't share the snapshot. Snapshot lives in SQLite, not in-memory, so any process reads the same row.
- **Tilt invariant from P11.** When P11 lands, `RiskEngine::size` must consult `tilt_guard.account_paused()` and return `Sizing::paused`. Phase 1 reserves the field (`sizing_skipped: "tilt_paused"` enum variant) but does not implement the check; P11 wires it.
- **Avoid touching `SetupCandidate`'s wire shape unnecessarily.** Add `sizing: Option<Sizing>` as the only new field; existing serialization paths (MCP, Tauri events) continue to round-trip with `null` for pre-P1 rows.
