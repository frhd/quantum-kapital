-- V21__event_blackouts.sql
-- Phase 5 (quant-decisions roadmap): event-blackout gate. The runner
-- refuses to fire a detector hit inside an asymmetric event window
-- (earnings or FOMC) for detectors that have configured a window.
-- Blackout-gated hits still land as `setups` rows so the trader can
-- see them in a "skipped today" panel and override per-setup with a
-- recorded reason.
--
-- 1. Two new columns on `setups`:
--    - `skipped_reason`   — short tag like 'earnings_blackout',
--                           'fomc_blackout', 'override_*'. NULL on
--                           pre-P5 rows AND on every non-skipped hit
--                           (which stays the typical case). Distinct
--                           from `sizing_skipped_reason` (P1) — that
--                           one tracks risk-engine sizing failures;
--                           this one tracks detector-level gate skips.
--    - `skip_window_json` — JSON `{ kind, start, end, reason, source,
--                           confidence }` describing the blackout the
--                           setup tripped. NULL when `skipped_reason`
--                           is NULL.
--    Pre-P5 rows are left at NULL — never "retroactively skipped".
--
-- 2. `event_calendar_cache` — earnings-date cache keyed by `symbol`.
--    Refreshed weekly per the master plan (AV daily quota). One row
--    per symbol; the `next_earnings_date` is the next *upcoming*
--    earnings announcement we know about. `confidence` is `'estimated'`
--    | `'confirmed'` so callers can widen the window when the date is
--    a guess.
--
-- 3. `event_calendar_overrides` — operator-curated earnings dates
--    that override the AV cache. Mirrors `manual_fundamentals` in
--    spirit: a row here always wins over the AV cache for that
--    symbol. Written via the MCP rail in a future phase; for P5 the
--    tests insert directly.
--
-- 4. `setup_blackout_overrides` — audit row written when the trader
--    chooses to take a blackout-skipped setup anyway. References
--    `setups.id`. The new (non-skipped) setup that the override
--    produces gets a fresh row in `setups`; `setup_blackout_overrides`
--    keys on the *original* skipped setup and records the new id,
--    actor, and free-text reason.

ALTER TABLE setups ADD COLUMN skipped_reason TEXT;
ALTER TABLE setups ADD COLUMN skip_window_json TEXT;

CREATE INDEX IF NOT EXISTS idx_setups_skipped_reason
    ON setups(skipped_reason)
    WHERE skipped_reason IS NOT NULL;

CREATE TABLE IF NOT EXISTS event_calendar_cache (
    symbol              TEXT PRIMARY KEY,
    next_earnings_date  TEXT NOT NULL,    -- ISO 8601 yyyy-mm-dd
    confidence          TEXT NOT NULL,    -- 'estimated' | 'confirmed'
    fetched_at          INTEGER NOT NULL, -- unix seconds
    source              TEXT NOT NULL     -- 'alpha_vantage' | 'manual'
);
CREATE INDEX IF NOT EXISTS idx_event_calendar_cache_fetched
    ON event_calendar_cache(fetched_at DESC);

CREATE TABLE IF NOT EXISTS event_calendar_overrides (
    symbol              TEXT PRIMARY KEY,
    next_earnings_date  TEXT NOT NULL,
    confidence          TEXT NOT NULL,
    written_at          INTEGER NOT NULL,
    written_by          TEXT NOT NULL,
    notes               TEXT
);

CREATE TABLE IF NOT EXISTS setup_blackout_overrides (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    skipped_setup_id    INTEGER NOT NULL REFERENCES setups(id) ON DELETE CASCADE,
    new_setup_id        INTEGER REFERENCES setups(id) ON DELETE SET NULL,
    gate_kind           TEXT NOT NULL,    -- 'earnings_blackout' | 'fomc_blackout'
    reason              TEXT NOT NULL,    -- non-empty free text from the trader
    actor               TEXT NOT NULL,    -- 'human' | 'agent:<name>'
    overridden_at       INTEGER NOT NULL  -- unix seconds
);
CREATE INDEX IF NOT EXISTS idx_setup_blackout_overrides_skipped
    ON setup_blackout_overrides(skipped_setup_id);
CREATE INDEX IF NOT EXISTS idx_setup_blackout_overrides_new
    ON setup_blackout_overrides(new_setup_id);
