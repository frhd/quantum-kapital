-- V17__risk_engine.sql
-- Phase 1 (quant-decisions roadmap): persistence shape for the new
-- `services/risk_engine/`. Two pieces:
--
-- 1. New columns on `setups` so each detector hit carries the qty,
--    dollar-risk, and per-share R the engine computed at decision
--    time. Pre-P1 rows stay NULL; the API surfaces NULL as "ungated"
--    so the frontend can render a placeholder rather than a phantom 0.
--    All money is stored as integer cents to avoid f64 round-trip
--    drift through SQLite REAL.
--
-- 2. `equity_snapshots` keyed by `(account, as_of_date)` (ET trading
--    date as ISO `YYYY-MM-DD`). The risk engine pins sizing to the
--    T-1 close NLV; one row per account per trading date is the
--    only thing we need. `source` records whether the snapshot came
--    from a fresh IBKR fetch, a stale-cache fallback, or a manual
--    override. The unique key prevents duplicate snapshots silently
--    racing during open-replay.

ALTER TABLE setups ADD COLUMN qty INTEGER;
ALTER TABLE setups ADD COLUMN dollar_risk_cents INTEGER;
ALTER TABLE setups ADD COLUMN r_per_share_cents INTEGER;
ALTER TABLE setups ADD COLUMN equity_at_decision_cents INTEGER;
ALTER TABLE setups ADD COLUMN sizing_version INTEGER;
ALTER TABLE setups ADD COLUMN sizing_skipped_reason TEXT;
ALTER TABLE setups ADD COLUMN conviction_grade TEXT;
ALTER TABLE setups ADD COLUMN conviction_multiplier_bps INTEGER;
ALTER TABLE setups ADD COLUMN sizing_cap_applied INTEGER;

CREATE TABLE IF NOT EXISTS equity_snapshots (
    account     TEXT NOT NULL,
    as_of_date  TEXT NOT NULL,
    nlv_cents   INTEGER NOT NULL,
    source      TEXT NOT NULL,
    fetched_at  INTEGER NOT NULL,
    PRIMARY KEY(account, as_of_date)
);
CREATE INDEX IF NOT EXISTS idx_equity_snapshots_account_fetched
    ON equity_snapshots(account, fetched_at DESC);
