-- V24__concentration_gate.sql
-- Phase 8 (quant-decisions roadmap): portfolio risk view + concentration
-- gate. The runner now consults a deterministic `ConcentrationGate`
-- before persisting a `SetupCandidate`; the master plan's cross-phase
-- "override audit" requirement lands in `gate_overrides` here.
--
-- Three surfaces extended:
--
--   1. `setups.gate_warning` — short tag like 'sector_80pct',
--      'name_80pct', 'total_80pct'. NULL on every non-warned hit
--      (the typical case). When the gate's severity ladder fires
--      `warn` (≥80% of a configured limit, < 100%), the setup is
--      persisted normally with this column set so the SetupCard can
--      render a banner. `block` (≥100%) lands as a skipped row with
--      `skipped_reason = 'concentration_blocked'` and
--      `skip_window_json` carrying the breach descriptor.
--
--   2. `gate_overrides` — append-only audit table backing the
--      master cross-phase verification gate. Every override (any gate
--      kind: blackout / concentration / regime / tilt) writes a row
--      here with `setup_id`, `gate_kind`, `reason`, `actor`, `at`.
--      Trader-profile rollup queries this table for "override frequency
--      by gate" — if any single gate exceeds 30% over 60 days the gate
--      is too strict OR the trader is rationalizing. Pre-P8 there was
--      no unified table; `setup_blackout_overrides` from V21 stays
--      immutable and is read alongside this table.
--
--   3. `portfolio_snapshots` — time-series of total open dollar-risk
--      and per-(sector|factor|name) exposure. Recomputed on
--      executions / bracket events and on a 60s tick (see
--      `services/portfolio_risk/`). UI renders both the current
--      value and the trailing series; `exposures_json` is a structured
--      payload so the historical view can replay the same heatmap
--      shape that drove a past gate decision.
--
-- All columns NULL-tolerant for back-compat. Older `setups` rows
-- read with `gate_warning = NULL` and the SetupCard suppresses the
-- banner. The tables CREATE IF NOT EXISTS so a re-run is idempotent.

ALTER TABLE setups ADD COLUMN gate_warning TEXT;

CREATE INDEX IF NOT EXISTS idx_setups_gate_warning
    ON setups(gate_warning)
    WHERE gate_warning IS NOT NULL;

CREATE TABLE IF NOT EXISTS gate_overrides (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    setup_id      INTEGER NOT NULL REFERENCES setups(id) ON DELETE CASCADE,
    -- 'concentration' | 'earnings_blackout' | 'fomc_blackout' |
    -- 'regime' | 'tilt'. Future-phase gates extend this set; the
    -- column stays free-text (no CHECK) so a new gate kind doesn't
    -- need a migration.
    gate_kind     TEXT NOT NULL,
    reason        TEXT NOT NULL,
    actor         TEXT NOT NULL,    -- 'human' | 'agent:<name>'
    at_unix       INTEGER NOT NULL  -- unix seconds, UTC
);

CREATE INDEX IF NOT EXISTS idx_gate_overrides_setup
    ON gate_overrides(setup_id);

CREATE INDEX IF NOT EXISTS idx_gate_overrides_kind_at
    ON gate_overrides(gate_kind, at_unix DESC);

CREATE TABLE IF NOT EXISTS portfolio_snapshots (
    id                          INTEGER PRIMARY KEY AUTOINCREMENT,
    account                     TEXT    NOT NULL,
    at_unix                     INTEGER NOT NULL,    -- unix seconds, UTC
    nlv_cents                   INTEGER NOT NULL,
    total_dollar_risk_cents     INTEGER NOT NULL,
    open_position_count         INTEGER NOT NULL,
    -- JSON payload: {
    --   "by_sector":  [{ "label": "tech", "dollar_risk_cents": 12345 }, ...],
    --   "by_factor":  [{ "label": "momentum_high", "count": 4 }, ...],
    --   "by_name":    [{ "symbol": "NVDA", "dollar_risk_cents": 12345,
    --                    "stop_estimated": false }, ...]
    -- }
    -- Stored as a single column so the snapshot row is a self-contained
    -- replay artifact for the gate decision audit.
    exposures_json              TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_portfolio_snapshots_account_at
    ON portfolio_snapshots(account, at_unix DESC);
