-- V27__tilt_episodes.sql
-- Phase 11 (quant-decisions roadmap): account-level tilt circuit
-- breaker. After a -3R day OR two consecutive losing closed trades,
-- new setup placement is paused for the rest of the session. Manual
-- override exists, is logged, and counts toward trader-profile.
--
-- Schema shape:
--
--   `tilt_episodes` — one row per session-pause cycle. Open rows have
--   `released_at_unix IS NULL`. Trigger details captured at activation
--   so trader-profile can roll up "tilt episodes per month" without
--   needing the underlying R-stream. The `release_kind` taxonomy:
--     - 'auto'            — released by the auto-reset at next session
--                           open (calendar-aware: Friday → Monday).
--     - 'manual_override' — trader hit the dismiss button with a reason.
--                           A `gate_overrides` row also lands with
--                           `gate_kind = 'tilt'` for cross-gate audit.
--     - 'session_end'     — defensive, when a follow-up activation on
--                           a new session closes a stale row that the
--                           auto-reset missed.
--
-- All gates that block a setup share `gate_overrides` (V24); tilt
-- overrides write there with `gate_kind = 'tilt'` AND flip the
-- `release_kind` here. Two surfaces, one audit trail.

CREATE TABLE IF NOT EXISTS tilt_episodes (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    account                  TEXT    NOT NULL,
    triggered_at_unix        INTEGER NOT NULL,                -- unix seconds, UTC
    -- 'cum_r_negative_3r' | 'two_consecutive_losses'.
    -- Free-text on purpose so a future trigger kind doesn't need a
    -- migration.
    trigger_kind             TEXT    NOT NULL,
    -- Cumulative R at activation (×1000 to keep integer math). The
    -- value is informational; future trader-profile views can render
    -- it as a fractional R.
    cumulative_r_milli       INTEGER NOT NULL,
    -- Consecutive-loss count at activation. 0 when the trigger was
    -- the cumulative-R rule.
    consecutive_losses       INTEGER NOT NULL,
    -- Auto-reset target — typically next 09:30 ET session open.
    -- Recorded so the UI banner can render "resumes 2026-05-07 09:30 ET"
    -- without recomputing the calendar on every render.
    auto_reset_at_unix       INTEGER NOT NULL,
    -- Set when the episode is closed; NULL while paused.
    released_at_unix         INTEGER,
    -- 'auto' | 'manual_override' | 'session_end'. NULL while open.
    release_kind             TEXT,
    -- Free-text reason captured from the override modal. NULL on
    -- auto release. Mirrored into `gate_overrides.reason` when the
    -- release_kind is `manual_override`.
    release_reason           TEXT
);

CREATE INDEX IF NOT EXISTS idx_tilt_episodes_account_open
    ON tilt_episodes(account, released_at_unix);

CREATE INDEX IF NOT EXISTS idx_tilt_episodes_account_trig
    ON tilt_episodes(account, triggered_at_unix DESC);
