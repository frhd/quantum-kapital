-- V26__param_refit.sql
-- Phase 10 (quant-decisions roadmap): walk-forward parameter refit.
-- Detector parameters become evidence-driven, monthly-locked vintages
-- rather than hand-picked settings.toml constants. settings.toml
-- transitions from active params to floor/ceiling bounds for the
-- sweep; the active params at runtime come from the latest non-
-- superseded `param_vintages` row per detector.
--
-- Two surfaces extended here:
--
--   1. `param_vintages` — append-only history of locked vintages. One
--      row per (detector, lock_event). The "active" vintage for a
--      detector is the most recent row where `superseded_at IS NULL`.
--      When a new candidate beats the current by ≥ 10% on the
--      objective AND meets all constraints, the lock writer:
--        a) UPDATE-stamps the prior active row's `superseded_at`,
--        b) INSERT-s the new winner with `superseded_at = NULL`.
--      Failed sweep attempts (constraints unmet, or didn't beat by
--      10%) do NOT write a row — only locked winners persist. The
--      `attempted_configs_json` audit array carries every config the
--      sweep tried so reviewers can spot luck-driven winners.
--
--   2. `setups.param_vintage_id` — TEXT pointer to the
--      `param_vintages.vintage_id` that was active when the setup
--      fired. Pre-P10 rows stay NULL; the eval panel suppresses the
--      vintage badge in that case. Threaded through TrackerRunner
--      from the active-vintage cache held by ParamRefitService.
--
-- vintage_id format: `vint_<detector>_<YYYYMMDD>_<short_hash>`.
-- Generated server-side at lock time so two vintages locked on the
-- same day for the same detector (rare, but possible during backfill)
-- collide on hash but not on full id.

ALTER TABLE setups ADD COLUMN param_vintage_id TEXT;

CREATE TABLE IF NOT EXISTS param_vintages (
    -- TEXT PK so the row is referenceable from `setups.param_vintage_id`
    -- without a join through an integer surrogate.
    vintage_id              TEXT PRIMARY KEY,
    -- Detector identifier. Matches `SetupCandidate.strategy`
    -- ("breakout", "episodic_pivot", "parabolic_short").
    detector                TEXT NOT NULL,
    -- JSON snapshot of the locked params. Shape is detector-specific
    -- (matches the `*Cfg` struct serialization). Migrating the schema
    -- of a detector's params is a manual op: stamp the existing rows
    -- as superseded and run a backfill refit so the new shape lands.
    params_json             TEXT NOT NULL,
    -- The objective value the locked candidate scored on its OOS
    -- window. Used by the next refit's lock-on-improvement check —
    -- new must beat this by ≥ 10% AND meet all hard constraints.
    objective_value         REAL NOT NULL,
    -- Number of OOS trades the candidate fired. Constraint guard:
    -- a vintage with < 30 OOS trades over 3 months is rejected
    -- regardless of objective value.
    oos_n_trades            INTEGER NOT NULL,
    -- ISO YYYY-MM-DD train + OOS window bounds (inclusive). Stored
    -- so the eval panel can render "trained 2026-04-01 → 2026-04-30,
    -- OOS 2026-05-01 → 2026-05-31".
    train_window_from       TEXT NOT NULL,
    train_window_to         TEXT NOT NULL,
    oos_window_from         TEXT NOT NULL,
    oos_window_to           TEXT NOT NULL,
    -- Unix seconds (UTC) for lock + supersede events. `superseded_at
    -- IS NULL` ⇒ this is the active vintage for `detector`.
    locked_at               INTEGER NOT NULL,
    superseded_at           INTEGER,
    -- 'cron' (monthly scheduler), 'manual' (admin lock), 'backfill'
    -- (one-shot run on first startup when no vintage exists for a
    -- detector). Distinguishes operator-initiated locks from the
    -- automated cadence so a UI filter can isolate "did the cron
    -- ever beat the manual override?".
    source                  TEXT NOT NULL,
    -- JSON array of {params, objective, n_trades, sharpe, expectancy}
    -- for every config the sweep evaluated. Keeps the multiple-
    -- comparisons audit visible: a winner that beat 199 other
    -- configs by 1% is suspicious; one that beat 5 configs by 30%
    -- is more credible. Empty array `[]` for backfill / manual rows
    -- that didn't run a sweep.
    attempted_configs_json  TEXT NOT NULL DEFAULT '[]',
    -- Optional human-readable note (e.g. "manual lock: trader
    -- requested wider RSI ceiling for next earnings window").
    notes                   TEXT
);

CREATE INDEX IF NOT EXISTS idx_param_vintages_detector_active
    ON param_vintages(detector) WHERE superseded_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_param_vintages_detector_locked_at
    ON param_vintages(detector, locked_at DESC);
